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

mod analysis;
pub mod custom;
pub mod dca;
mod indicators;
pub mod optimize;
pub mod params;
pub mod report;

pub use custom::CustomIndicatorDef;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Engine semantics version. Stamped into every run result and persisted on save so that
/// when the simulation's meaning changes later, old saved runs stay explainable. **Bump this
/// whenever a change alters the trades/equity a given settings JSON produces** (the golden
/// tests are the tripwire that tells you a bump is due).
///
/// v2: stop-and-reverse entries now size correctly under risk-based sizing (previously
/// they were silently skipped); grid trades carry their entry fee in pnl/fees; sizing and
/// exposure limits evaluate against marked-to-market equity (was: realized cash), and the
/// margin check runs against equity net of margin already committed by open positions.
///
/// v3: sizing accounts for the contract multiplier (percent-equity, risk, Kelly and equity
/// tiers previously sized a x50 contract 50x too large, and risk-sized entries were then
/// refused wholesale by the margin check); a stop touched by a bar that gapped past it fills
/// at that bar's open instead of at the stop price; exposure caps count the lot being opened,
/// so a cap can no longer be breached by one full position; breakeven trades are their own
/// category instead of counting as wins; profit factor is +inf rather than 0 when there are no
/// losses; Sharpe/Sortino return None on float-noise variance instead of ~1e14; and the
/// out-of-sample split attributes a trade to the segment it closed in.
///
/// v4: the DCA time-weighted curve chains a contribution row over the two halves the money
/// splits it into (close to open on the capital already invested, open to close on everything
/// present once the contribution has filled). It used to subtract the contribution from the
/// closing value, which credited the new money's own intraday gain to the capital already
/// there, so the TWR, Sharpe, Sortino and the percent drawdown of a savings plan were all
/// overstated in its first months. No money figure moves: the equity curve, the trades and the
/// currency drawdown are untouched, and no other engine has an external flow to neutralise.
pub const ENGINE_VERSION: u32 = 4;

/// A price/indicator series a signal can reference.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
    /// A derived price statistic, in percent: "dd_from_high" (below the running high of close,
    /// ≥ 0), "up_from_low" (above the running low, ≥ 0), "change_pct" (over `period` bars),
    /// "change_from_start". Written for the DCA rules ("buy 20% off the high") but valid in
    /// any signal group.
    Metric {
        metric: String,
        #[serde(default)]
        period: usize,
    },
    /// Live portfolio state, readable only inside a DCA run: "pnl_pct" | "since_last_buy_pct" |
    /// "avg_cost" | "units" | "value" | "weight_pct" | "cash_pct" | "drawdown_pct". Anywhere
    /// else it is undefined on every bar, so a signal referencing it never holds.
    Position { field: String },
}

/// Embedded custom-indicator definitions for a run, keyed by indicator id. Strategy settings
/// carry the defs they use so a saved run never changes meaning when the library is edited.
pub type IndicatorDefs = std::collections::BTreeMap<String, custom::CustomIndicatorDef>;

/// The comparison a signal makes between a `left` and `right` operand.
/// `cross` = left crosses above OR below; directionless crosses use `crosses_above`/below.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, schemars::JsonSchema)]
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

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Signal {
    pub left: Operand,
    pub op: Op,
    /// Right operand. Optional for unary ops (rising/falling on `left`).
    #[serde(default)]
    pub right: Option<Operand>,
}

/// A combination of signal conditions: ALL must hold ("all") or ANY may hold ("any").
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SignalGroup {
    #[serde(default = "default_logic")]
    pub logic: String,
    #[serde(default)]
    pub conditions: Vec<Signal>,
}
fn default_logic() -> String {
    "all".into()
}
impl Default for SignalGroup {
    /// An empty AND group: it holds nothing, so a rule that carries one never fires.
    fn default() -> Self {
        SignalGroup { logic: default_logic(), conditions: Vec::new() }
    }
}

/// One row of an equity-tier table: at/above this equity, use `value`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Tier {
    pub above: f64,
    pub value: f64,
}

/// How to size each entry (each pyramiding add is sized independently by the same rule).
/// Sizing is evaluated against **marked-to-market portfolio equity** at entry time
/// (cash + unrealized PnL of every open position).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Sizing {
    /// Percent of current equity put into the position notional.
    PercentEquity { percent: f64 },
    /// Fixed quantity / lots / contracts (units of the instrument), multiplied by leverage
    /// (retail-exchange convention: the input is what you commit, leverage scales exposure).
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
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Long,
    Short,
    Both,
}

/// A stop-loss / take-profit distance rule. `pct` = fraction of the average entry price (today's
/// behavior); `atr` = a multiple of ATR(`period`) sampled at the position's first-entry bar, held
/// as an absolute price distance and re-anchored to the average entry when pyramiding adds.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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

/// A trailing stop. The level ratchets **at each closed candle** (from that candle's favorable
/// extreme) and never loosens; the exit itself is the same intrabar touch as `stop_loss`, filled
/// with the same gap rule. Recomputing it only on a closed candle is what makes the level
/// lookahead-free: it is never derived from the bar it is then tested against. A future live
/// mode adds a per-tick variant of the *update*, not of the trigger.
///
/// `pct` = fraction of the extreme it follows (a 2% trailing stop), `abs` = an absolute price
/// distance in whatever the instrument quotes (points, pips, currency). Those two are the whole
/// vocabulary: a distance that reads an indicator is an indicator, so an ATR trail belongs in an
/// exit condition, not in this rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Trail {
    /// "pct" | "abs"
    #[serde(default = "default_stop_kind")]
    pub kind: String,
    /// Fraction (pct) or absolute price distance (abs). 0 disables.
    #[serde(default)]
    pub value: f64,
    /// Arm the trail only once the trade is this far in profit, as a fraction of the average
    /// entry (0.01 = 1%). 0 = live from the entry bar. Below the threshold the position is held
    /// by the fixed stop-loss alone, if it has one.
    #[serde(default)]
    pub activate_pct: f64,
    /// Move the stop to the average entry (gross breakeven, fees excluded) once the trade is
    /// this far in profit, as a fraction of the average entry. 0 = off. Independent of
    /// `activate_pct`: a plan can go to breakeven at 1% and only start trailing at 3%.
    #[serde(default)]
    pub breakeven_pct: f64,
}
/// `derive(Default)` would give the kind an empty string, which is not one of the two the rule
/// accepts: a bare breakeven step (`Trail { breakeven_pct, ..default() }`) would then be refused
/// for its kind rather than for what it is.
impl Default for Trail {
    fn default() -> Trail {
        Trail { kind: default_stop_kind(), value: 0.0, activate_pct: 0.0, breakeven_pct: 0.0 }
    }
}

impl Trail {
    /// Price distance to hold behind `anchor` (the candle's favorable extreme), or None when the
    /// rule is off.
    fn distance(&self, anchor: f64) -> Option<f64> {
        if self.value <= 0.0 {
            return None;
        }
        match self.kind.as_str() {
            "abs" => Some(self.value),
            _ => Some(anchor.abs() * self.value),
        }
    }
    fn validate(&self) -> Option<String> {
        if self.kind == "atr" {
            return Some(
                "a trailing stop is a percent or a price distance; an ATR trail is an indicator, \
                 build it in the indicator library and exit on it"
                    .into(),
            );
        }
        if !matches!(self.kind.as_str(), "pct" | "abs") {
            return Some(format!("trailing stop kind must be pct or abs (got '{}')", self.kind));
        }
        if self.value < 0.0 {
            return Some("trailing stop distance cannot be negative".into());
        }
        if self.activate_pct < 0.0 || self.breakeven_pct < 0.0 {
            return Some("trailing stop activation and breakeven thresholds cannot be negative".into());
        }
        None
    }
}

/// Per-side configuration. A side is only consulted when the `Mode` enables it.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
    /// Trailing stop. Independent of `stop_loss`: when both are set the tighter of the two is
    /// the level the market reaches first, and it names the exit reason.
    #[serde(default)]
    pub trailing_stop: Option<Trail>,
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
    /// Effective trailing-stop rule, only when it actually does something. A distance of 0 with
    /// a breakeven step is a legal plan ("move the stop to entry at +1%, never trail"), so the
    /// two are tested separately.
    fn trail_rule(&self) -> Option<Trail> {
        self.trailing_stop.clone().filter(|t| t.value > 0.0 || t.breakeven_pct > 0.0)
    }
    /// A trailing stop that is live from the entry bar, so its first level (`entry ∓ distance`)
    /// is known *before* the position is sized. One that waits for `activate_pct`, or one that
    /// only steps to breakeven later, is not: there is no stop price at entry to size against.
    fn trail_at_entry(&self) -> Option<Trail> {
        self.trail_rule().filter(|t| t.value > 0.0 && t.activate_pct <= 0.0)
    }
    /// Whether this side has *any* stop-loss configured (for risk-sizing validation).
    fn has_stop(&self) -> bool {
        self.sl_rule().is_some() || self.trail_at_entry().is_some()
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
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
    /// Centre the ladder on a moving reference instead of the fixed `[lower, upper]` band:
    /// "none" (default) | "sma" | "ema" | "dema" | "tema" | "wma" | "hma" | "vwap". The band
    /// then follows that line bar by bar, `lower`/`upper` are ignored, and the ladder only
    /// starts once the reference (and its width) exist; those leading bars are the warm-up.
    #[serde(default = "default_grid_anchor")]
    pub anchor: String,
    #[serde(default = "default_grid_anchor_period")]
    pub anchor_period: usize,
    /// Half-width of an anchored ladder: "pct" of the anchor (2 = ±2%) or "atr" multiples.
    #[serde(default = "default_grid_width_kind")]
    pub width_kind: String,
    #[serde(default = "default_grid_width_value")]
    pub width_value: f64,
    /// ATR period backing `width_kind == "atr"`.
    #[serde(default = "default_grid_width_period")]
    pub width_period: usize,
    /// Rebuild the ladder on every bar close: open cells are settled at that close (exit
    /// reason `grid_reset`) and the next bar trades a ladder centred on the new anchor.
    #[serde(default)]
    pub reset_on_close: bool,
}
fn default_grid_levels() -> usize {
    10
}
fn default_grid_direction() -> String {
    "long".into()
}
fn default_grid_anchor() -> String {
    "none".into()
}
fn default_grid_anchor_period() -> usize {
    20
}
fn default_grid_width_kind() -> String {
    "pct".into()
}
fn default_grid_width_value() -> f64 {
    2.0
}
fn default_grid_width_period() -> usize {
    14
}
impl Default for GridConfig {
    /// A fixed, unanchored ladder over `[0, 0]`: callers set the band (or the anchor) they mean.
    fn default() -> Self {
        Self {
            lower: 0.0,
            upper: 0.0,
            levels: default_grid_levels(),
            qty_per_level: 0.0,
            total_budget: 0.0,
            direction: default_grid_direction(),
            stop_below: 0.0,
            stop_above: 0.0,
            anchor: default_grid_anchor(),
            anchor_period: default_grid_anchor_period(),
            width_kind: default_grid_width_kind(),
            width_value: default_grid_width_value(),
            width_period: default_grid_width_period(),
            reset_on_close: false,
        }
    }
}

/// One intraday trading window on the filters' local clock, `"HH:MM"` both ends. The end is
/// exclusive (09:30–16:00 lets a 16:00 bar's signal fire but not fill on it), and a window whose
/// end is *before* its start wraps past midnight (22:00–02:00 is one Asian session, not none).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Session {
    pub from: String,
    pub to: String,
}

impl Session {
    /// Does `m` (minutes since local midnight) fall in this window? An unparseable window
    /// constrains nothing: `validate` rejects those before a run ever reaches here.
    fn contains(&self, m: u16) -> bool {
        match (hhmm(&self.from), hhmm(&self.to)) {
            (Some(a), Some(b)) if a < b => m >= a && m < b,
            (Some(a), Some(b)) if a > b => m >= a || m < b,
            _ => true,
        }
    }
}

/// A calendar rule: one local date, or a closed range when `to` is set (both ends inclusive).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DateRange {
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
}

impl DateRange {
    fn contains(&self, d: time::Date) -> bool {
        let Some(a) = ymd(&self.from) else { return false };
        let b = self.to.as_deref().and_then(ymd).unwrap_or(a);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        d >= a && d <= b
    }
}

/// "HH:MM" → minutes since midnight.
fn hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// "YYYY-MM-DD" → a date.
fn ymd(s: &str) -> Option<time::Date> {
    time::Date::parse(s.trim(), time::macros::format_description!("[year]-[month]-[day]")).ok()
}

/// When the strategy is allowed to open. Bars are stored in UTC; every rule below reads the
/// timestamp shifted by `tz_offset_min`, which is the exchange/session clock the user works in.
///
/// Filters gate **new entries only**: exits, stops, take-profits, funding and marking-to-market
/// run on every bar, so a filter can never trap an open position. They also never trim bars:
/// indicators keep computing across a closed window, so switching a filter on doesn't shift an
/// EMA by one bar (that is the difference with narrowing the run's date window).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Filters {
    /// Minutes east of UTC for every rule here (+120 = UTC+2). A fixed offset, not a timezone:
    /// no DST shift is applied, which is exactly what a "session runs 09:30–16:00 UTC-5" rule
    /// means to the person writing it.
    #[serde(default)]
    pub tz_offset_min: i32,
    /// Allowed weekdays, 1 = Monday … 7 = Sunday. Empty = every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// Allowed intraday windows. Empty = the whole day; several windows are OR-ed.
    #[serde(default)]
    pub sessions: Vec<Session>,
    /// When non-empty, entries are allowed *only* on these local dates.
    #[serde(default)]
    pub include_dates: Vec<DateRange>,
    /// Never enter on these local dates (wins over `include_dates`).
    #[serde(default)]
    pub exclude_dates: Vec<DateRange>,
    /// What an open position does when the window closes: "hold" (default, it keeps managing
    /// its own exits) | "flat" (closed at the open of the first bar outside the window,
    /// exit reason `session_end`).
    #[serde(default = "default_window_end")]
    pub on_window_end: String,
    /// Whether a closed window also blocks pyramiding adds (default true). Off = the window
    /// governs opening a position, and an existing one may still be built out.
    #[serde(default = "default_true")]
    pub block_adds: bool,
}
fn default_window_end() -> String {
    "hold".into()
}
fn default_true() -> bool {
    true
}
impl Default for Filters {
    fn default() -> Self {
        Filters {
            tz_offset_min: 0,
            weekdays: Vec::new(),
            sessions: Vec::new(),
            include_dates: Vec::new(),
            exclude_dates: Vec::new(),
            on_window_end: default_window_end(),
            block_adds: true,
        }
    }
}

impl Filters {
    /// Whether any rule is set. Inert filters skip the per-row work entirely.
    fn is_active(&self) -> bool {
        !self.weekdays.is_empty()
            || !self.sessions.is_empty()
            || !self.include_dates.is_empty()
            || !self.exclude_dates.is_empty()
    }

    fn flat_on_close(&self) -> bool {
        self.on_window_end == "flat"
    }

    /// Structural check surfaced before the run. A malformed rule is refused rather than
    /// silently ignored: "the filter did nothing" is the one failure the user cannot see.
    fn validate(&self) -> Option<String> {
        if !(-14 * 60..=14 * 60).contains(&self.tz_offset_min) {
            return Some("filters: UTC offset must be between -14:00 and +14:00".into());
        }
        if let Some(d) = self.weekdays.iter().find(|d| !(1..=7).contains(*d)) {
            return Some(format!("filters: weekday {d} is not in 1..7 (1 = Monday)"));
        }
        for s in &self.sessions {
            match (hhmm(&s.from), hhmm(&s.to)) {
                (Some(a), Some(b)) if a == b => {
                    return Some(format!(
                        "filters: session {} - {} starts and ends at the same time",
                        s.from, s.to
                    ))
                }
                (Some(_), Some(_)) => {}
                _ => {
                    return Some(format!(
                        "filters: session {} - {} must be two HH:MM times",
                        s.from, s.to
                    ))
                }
            }
        }
        for r in self.include_dates.iter().chain(&self.exclude_dates) {
            for d in [Some(r.from.as_str()), r.to.as_deref()].into_iter().flatten() {
                if ymd(d).is_none() {
                    return Some(format!("filters: invalid date \"{d}\", use YYYY-MM-DD"));
                }
            }
        }
        if !matches!(self.on_window_end.as_str(), "hold" | "flat") {
            return Some("filters: `on_window_end` must be \"hold\" or \"flat\"".into());
        }
        None
    }

    /// May the strategy open on the bar stamped `ts`? An unparseable timestamp opens the gate:
    /// a clock rule is a restriction the user asked for on data it can read, never a silent halt.
    fn allows(&self, ts: &str, off: time::UtcOffset) -> bool {
        use time::{format_description::well_known::Rfc3339, OffsetDateTime};
        let Ok(dt) = OffsetDateTime::parse(ts, &Rfc3339) else { return true };
        let local = dt.to_offset(off);
        if !self.weekdays.is_empty()
            && !self.weekdays.contains(&local.weekday().number_from_monday())
        {
            return false;
        }
        let date = local.date();
        if !self.include_dates.is_empty() && !self.include_dates.iter().any(|r| r.contains(date)) {
            return false;
        }
        if self.exclude_dates.iter().any(|r| r.contains(date)) {
            return false;
        }
        if !self.sessions.is_empty() {
            let m = local.hour() as u16 * 60 + local.minute() as u16;
            if !self.sessions.iter().any(|s| s.contains(m)) {
                return false;
            }
        }
        true
    }
}

/// Per-row "may open" flags for a clock, or None when the filters are inert (the hot path then
/// skips the lookup altogether). Computed once per run: the gate is a property of the clock,
/// not of an asset, so a portfolio run shares one verdict per row.
fn clock_gate(f: &Filters, clock: &[String]) -> Option<Vec<bool>> {
    if !f.is_active() {
        return None;
    }
    let secs = f.tz_offset_min.clamp(-14 * 60, 14 * 60) * 60;
    let off = time::UtcOffset::from_whole_seconds(secs).unwrap_or(time::UtcOffset::UTC);
    Some(clock.iter().map(|t| f.allows(t, off)).collect())
}

/// Full run configuration. Mirrors the settings JSON the frontend posts and stores.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Settings {
    /// Strategy kind: "signals" (default, the signal-combination engine) | "grid".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Grid config, required when `kind == "grid"`.
    #[serde(default)]
    pub grid: Option<GridConfig>,
    /// DCA plan, required when `kind == "dca"`.
    #[serde(default)]
    pub dca: Option<dca::DcaConfig>,
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
    /// When the strategy may open: session clock, weekdays, calendar. Unset = always.
    #[serde(default)]
    pub filters: Filters,
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
        // A DCA plan never reads `sizing`: its size is the weights and the tranches. Rejecting
        // a plan over a risk-based sizing mode it ignores would refuse a legal run.
        if self.kind != "dca" && sizing_needs_stop(&self.sizing) {
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
        for (label, side) in [("long", self.long.as_ref()), ("short", self.short.as_ref())] {
            if let Some(err) = side.and_then(|sd| sd.trailing_stop.as_ref()).and_then(Trail::validate) {
                return Some(format!("{label}: {err}"));
            }
        }
        // Every embedded custom-indicator definition must be structurally valid.
        for (id, def) in &self.indicators {
            if let Some(err) = def.validate() {
                return Some(format!("custom indicator '{id}': {err}"));
            }
        }
        if let Some(err) = self.filters.validate() {
            return Some(err);
        }
        if self.kind == "dca" {
            match &self.dca {
                None => return Some("dca mode needs a `dca` plan".into()),
                Some(cfg) => {
                    if let Some(err) = cfg.validate() {
                        return Some(err);
                    }
                }
            }
            // A savings plan has no in-sample / out-of-sample split (the engine produces no
            // `oos` block for it). Accepting the setting and ignoring it left the UI showing
            // nothing where the user asked for a split.
            if self.oos_split_pct != 0.0 {
                return Some(
                    "a dca run has no out-of-sample split: clear it, or compare two date windows"
                        .into(),
                );
            }
        }
        None
    }

    /// Configurations that run, produce plausible-looking statistics, and answer a different
    /// question than the one asked. Not errors — the settings are legal and someone may mean
    /// them — so they ride back with the result instead of rejecting it.
    ///
    /// The one that matters: `exit_on_reverse` closes the position as soon as the entry group
    /// stops holding, and an event operator (`crosses_above`, `crosses_below`, `cross`) holds
    /// only on the bar of the event. Pair the two with no explicit exit and every trade lasts
    /// exactly one bar — a result that looks like a strategy and measures nothing.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.kind == "dca" {
            if let Some(cfg) = &self.dca {
                out.extend(cfg.warnings());
            }
            return out;
        }
        let sides = [("long", self.long.as_ref()), ("short", self.short.as_ref())];
        for (label, side) in sides {
            let Some(side) = side else { continue };
            let enabled = match label {
                "long" => self.allow_long(),
                _ => self.allow_short(),
            };
            if !enabled || !side.exit_on_reverse {
                continue;
            }
            let entry_is_event = side
                .entry_group()
                .map(|g| {
                    !g.conditions.is_empty()
                        && g.conditions.iter().all(|c| {
                            matches!(c.op, Op::CrossesAbove | Op::CrossesBelow | Op::Cross)
                        })
                })
                .unwrap_or(false);
            let no_exit = side.exit.as_ref().is_none_or(|g| g.conditions.is_empty());
            if entry_is_event && no_exit {
                out.push(format!(
                    "{label}: `exit_on_reverse` with a crossover entry and no exit condition \
                     closes every position on the bar after it opens (expect avg_bars_held ≈ 1). \
                     Give the side an explicit exit condition, or use a state operator \
                     (above/below/rising/falling) for the entry."
                ));
            }
        }
        out
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

/// One asset's OHLCV owned in exactly the shape `Bars` borrows. The API loads a dataset into
/// this; a long-lived caller (the optimizer, which reads the same bars for every trial) holds it
/// for the life of the job so the window can never drift between simulations.
pub struct OwnedBars {
    pub ticker: String,
    pub timeframe: String,
    pub ts: Vec<String>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

impl OwnedBars {
    pub fn as_bars(&self) -> Bars<'_> {
        Bars {
            ticker: &self.ticker,
            ts: &self.ts,
            open: &self.open,
            high: &self.high,
            low: &self.low,
            close: &self.close,
            volume: &self.volume,
        }
    }
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
    /// Trades that closed at exactly zero PnL — neither a win nor a loss, so
    /// `wins + losses + breakeven == trades`.
    #[serde(default)]
    pub breakeven: usize,
}

/// Profit factor reported when a scope has winning trades and **no losing trades**, where the
/// true ratio is unbounded. A finite stand-in is required: JSON cannot carry infinity (serde
/// writes `null`, which the UI reads as "absent"), and the optimizer's ranking sinks non-finite
/// metrics to the bottom. Large enough to sort above any real ratio, small enough to survive
/// f32 truncation in the optimizer's row storage.
pub const PROFIT_FACTOR_NO_LOSSES: f64 = 1.0e9;

fn side_stats<'a>(trades: impl Iterator<Item = &'a Trade>) -> SideStats {
    let mut s = SideStats::default();
    let (mut streak_w, mut streak_l, mut bars, mut ret_sum) = (0usize, 0usize, 0usize, 0.0);
    for t in trades {
        s.trades += 1;
        s.net_pnl += t.pnl;
        s.total_fees += t.fees;
        bars += t.bars_held;
        ret_sum += t.return_pct;
        // Three-way, matching `analysis.rs` (the report) and `analysis/trades.js` (the UI):
        // an exactly-zero trade is neither a win nor a loss, and it breaks BOTH streaks rather
        // than extending the winning one.
        if t.pnl > 0.0 {
            s.wins += 1;
            s.gross_profit += t.pnl;
            s.largest_win = s.largest_win.max(t.pnl);
            streak_w += 1;
            streak_l = 0;
        } else if t.pnl < 0.0 {
            s.losses += 1;
            s.gross_loss += -t.pnl;
            s.largest_loss = s.largest_loss.max(-t.pnl);
            streak_l += 1;
            streak_w = 0;
        } else {
            s.breakeven += 1;
            streak_w = 0;
            streak_l = 0;
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
    // Profit factor. With no losing trades the ratio is unbounded. It cannot be reported as
    // `f64::INFINITY`: JSON has no infinity, so serde emits `null`, the UI's `!= null` guards
    // then hide the field entirely, and `optimize::cmp_metric` SINKS non-finite values — which
    // would rank a flawless trial last, the very bug this replaces. Report a finite sentinel
    // instead, so it serializes, displays, and sorts as the best result it is.
    s.profit_factor = if s.gross_loss > 0.0 {
        s.gross_profit / s.gross_loss
    } else if s.gross_profit > 0.0 {
        PROFIT_FACTOR_NO_LOSSES
    } else {
        0.0
    };
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
    // Guard on a RELATIVE epsilon, not `> 0.0`. A perfectly smooth compounding curve has
    // mathematically constant returns (variance 0, Sharpe undefined), but in floating point the
    // variance lands around 1e-32 rather than exactly zero — enough to pass a `> 0` test and
    // divide into a Sharpe of ~1e14. That degenerate value then sorts FIRST in an optimizer
    // ranking and poisons the deflated-Sharpe haircut computed from these same numbers.
    let degenerate = |v: f64| !(v.sqrt() > m.abs() * 1e-9 && v > f64::MIN_POSITIVE);
    let sharpe = (!degenerate(var)).then(|| m / var.sqrt() * ann);
    let sortino = (!degenerate(dvar)).then(|| m / dvar.sqrt() * ann);
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
    /// The period granularity rows are keyed on (`"1d"`), or None when timestamps had to
    /// match to the second (intraday data, or a set too irregular to read a granularity off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grain: Option<String>,
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
    /// Entries refused because the sized lot would push the book past an exposure cap.
    #[serde(default)]
    pub skipped_exposure: usize,
    /// Merged-clock rows where new entries were halted by a circuit breaker.
    pub halted_bars: usize,
    /// Merged-clock rows outside the trading window (session / weekday / date filters).
    pub filtered_bars: usize,
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
    /// DCA-specific stats, present only for a dca-kind run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dca: Option<dca::DcaStats>,
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
        Operand::Metric { metric, period } => metric_series(metric, *period, b),
        // Portfolio state has no meaning without a portfolio: the DCA engine resolves this one
        // itself, and every other caller sees an undefined series.
        Operand::Position { .. } => vec![None; b.close.len()],
    }
}

/// Derived price statistics, in percent. Running extremes walk forward over the closes only
/// (a new high reads 0, never a negative drawdown), so nothing here can see the future.
fn metric_series(metric: &str, period: usize, b: &Bars) -> Vec<Option<f64>> {
    let n = b.close.len();
    let mut out = vec![None; n];
    match metric {
        "dd_from_high" => {
            let mut hi = f64::NEG_INFINITY;
            for i in 0..n {
                hi = hi.max(b.close[i]);
                if hi > 0.0 {
                    out[i] = Some((hi - b.close[i]) / hi * 100.0);
                }
            }
        }
        "up_from_low" => {
            let mut lo = f64::INFINITY;
            for i in 0..n {
                lo = lo.min(b.close[i]);
                if lo > 0.0 {
                    out[i] = Some((b.close[i] - lo) / lo * 100.0);
                }
            }
        }
        "change_pct" => {
            let p = period.max(1);
            for i in p..n {
                if b.close[i - p] > 0.0 {
                    out[i] = Some((b.close[i] / b.close[i - p] - 1.0) * 100.0);
                }
            }
        }
        "change_from_start" => {
            if b.close.first().copied().unwrap_or(0.0) > 0.0 {
                let base = b.close[0];
                for i in 0..n {
                    out[i] = Some((b.close[i] / base - 1.0) * 100.0);
                }
            }
        }
        _ => {}
    }
    out
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
    /// Trailing-stop level, ratcheted at each closed candle and never loosened. None until the
    /// trail arms (no rule, or `activate_pct` not reached yet).
    trail_px: Option<f64>,
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
            trail_px: None,
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
    trail: Option<Trail>,
    /// ATR series for the SL/TP period, precomputed once when either rule is ATR-based.
    atr: Option<Vec<Option<f64>>>,
    exit_on_reverse: bool,
}

fn side_signals(side: Option<&Side>, enabled: bool, b: &Bars, defs: &IndicatorDefs) -> SideSignals {
    let side = side.filter(|_| enabled);
    let sl = side.and_then(|sd| sd.sl_rule());
    let tp = side.and_then(|sd| sd.tp_rule());
    let trail = side.and_then(|sd| sd.trail_rule());
    // If the SL or the TP is ATR-based, precompute ATR at its period. One series serves both, so
    // the longer period wins when they disagree. The trail has no ATR kind.
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
        trail,
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

/// Mark-to-market portfolio equity: cash plus every open position's unrealized PnL at its
/// asset's last-known close. This is the "equity" sizing rules and limits evaluate against.
fn mtm_equity(cash: f64, assets: &[Asset], mult: f64) -> f64 {
    let mut eq = cash;
    for a in assets {
        if let Some(p) = &a.pos {
            eq += p.unrealized(a.last_close, mult);
        }
    }
    eq
}

/// Margin already committed by open positions (open notional / leverage), so a new entry's
/// margin check runs against the *free* portion of equity rather than the whole of it.
fn committed_margin(assets: &[Asset], mult: f64, lev: f64) -> f64 {
    assets
        .iter()
        .filter_map(|a| a.pos.as_ref().map(|p| p.notional(a.last_close, mult)))
        .sum::<f64>()
        / lev.max(1e-9)
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
    /// Contract multiplier. Notional is `qty × price × multiplier` and per-unit risk is
    /// `|entry − stop| × multiplier`, so every sizing rule that reasons in account currency
    /// must divide by it — otherwise a x50 contract is sized 50x too large.
    mult: f64,
    closed: &'a [ClosedTrade],
}

/// Resolve the per-entry quantity for a sizing mode. Returns 0 to skip the entry. `sizing` is
/// evaluated against portfolio `equity`; risk modes need `stop_px` (validated up-front).
fn resolve_qty(sizing: &Sizing, c: &SizeCtx) -> f64 {
    let lev = if c.leverage > 0.0 { c.leverage } else { 1.0 };
    let mult = if c.mult > 0.0 { c.mult } else { 1.0 };
    // Units that carry `notional` of exposure at this fill price, multiplier included.
    let units_for = |notional: f64| if c.entry_px > 0.0 { notional / (c.entry_px * mult) } else { 0.0 };
    match sizing {
        Sizing::PercentEquity { percent } => units_for(c.equity * (percent / 100.0) * c.leverage),
        Sizing::FixedQty { qty } => qty * lev,
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
                "percent_equity" => units_for(c.equity * (val / 100.0) * c.leverage),
                _ => val * lev, // "qty" (default): fixed lots × leverage, like FixedQty
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
            units_for(c.equity * frac * c.leverage)
        }
    }
}

/// Quantity so that `|entry − stop| × qty = risk_pct% of equity`. Needs a stop price.
fn qty_for_risk(risk_pct: f64, c: &SizeCtx) -> f64 {
    let Some(stop) = c.stop_px else { return 0.0 };
    let mult = if c.mult > 0.0 { c.mult } else { 1.0 };
    // Risk per unit in ACCOUNT CURRENCY, which is what `risk_pct` is a percentage of.
    let per_unit = (c.entry_px - stop).abs() * mult;
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
fn op_lookback(o: &Operand, defs: &IndicatorDefs) -> usize {
    match o {
        Operand::Indicator { period, fast, slow, signal_period, .. } => {
            // Conservative upper bound: the largest configured length (defaults folded in).
            [*period, *fast, *slow, *signal_period].into_iter().max().unwrap_or(0)
        }
        Operand::CustomIndicator { id } => defs.get(id).map(|d| d.lookback()).unwrap_or(0),
        Operand::Metric { metric, period } => {
            if metric == "change_pct" {
                (*period).max(1)
            } else {
                0
            }
        }
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

fn warmup_bars(s: &Settings) -> usize {
    let mut w = 0;
    if let Some(cfg) = &s.dca {
        w = w.max(dca::dca_warmup(cfg, &s.indicators));
    }
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

/// Build the merged clock (every period any asset has) plus, for each asset, a row→bar-index
/// map. Rows are keyed on the **period** a bar belongs to, not on its raw timestamp, so two
/// providers stamping the same daily close differently (Binance at 00:00 UTC, Alpaca at the
/// New York session start) land on one row instead of two half-empty ones. See
/// [`crate::align`]; a single-asset or single-provider run gets back exactly the clock it
/// always had.
fn merged_clock(assets: &[&Bars]) -> (Vec<String>, Vec<Vec<Option<usize>>>) {
    let merged = align_assets(assets);
    (merged.clock, merged.maps)
}

/// The merge above, whole, for the alignment report (which also wants the granularity it
/// settled on and the fully-aligned span).
fn align_assets(assets: &[&Bars]) -> crate::align::Aligned {
    let series: Vec<crate::align::Series<'_>> = assets
        .iter()
        .map(|a| crate::align::Series { label: a.ticker, ts: a.ts })
        .collect();
    let mut merged =
        crate::align::merge(&series, crate::align::Grain::Infer, crate::align::Join::Union);
    // A bar that shares a period with another of the same asset's bars would be dropped from
    // the simulation. The inferred granularity is read off the data itself, so this only
    // happens on irregular input: fall back to exact timestamps rather than lose a bar.
    if merged.collapsed_total() > 0 {
        merged =
            crate::align::merge(&series, crate::align::Grain::Exact, crate::align::Join::Union);
    }
    merged
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
    let merged = align_assets(assets);
    let (clock, maps, grain) = (&merged.clock, &merged.maps, merged.grain);
    let asset_reports: Vec<AssetAlignment> = assets
        .iter()
        .zip(maps)
        .map(|(a, m)| AssetAlignment {
            ticker: a.ticker.to_string(),
            bars: a.ts.len(),
            first_ts: a.ts.first().cloned(),
            last_ts: a.ts.last().cloned(),
            inactive_bars: m.iter().filter(|x| x.is_none()).count(),
        })
        .collect();
    // Overlap = rows where every asset has a bar.
    let full = merged.overlap_rows();
    AlignmentReport {
        clock_len: clock.len(),
        overlap_from: full.first().map(|&r| clock[r].clone()),
        overlap_to: full.last().map(|&r| clock[r].clone()),
        overlap_bars: full.len(),
        warmup_bars: warmup_bars(s),
        grain: grain.map(|g| g.label()),
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
    // Grid and DCA are separate order generators, dispatched here (grid is single-asset:
    // it ladders one instrument, so it reads the first dataset).
    if s.kind == "grid" {
        return run_grid(s, inputs[0]);
    }
    if s.kind == "dca" {
        return dca::run_dca(s, inputs);
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
    let mut skipped_exposure = 0usize;
    let mut halted_bars = 0usize;
    // Trading-window gate (session clock / weekdays / dates): one verdict per merged-clock row.
    let gate = clock_gate(&s.filters, &clock);
    let flat_on_close = s.filters.flat_on_close();
    let block_adds = s.filters.block_adds;
    let mut filtered_bars = 0usize;
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
        // Is the trading window open on this row? (Exits never consult it.)
        let window_open = gate.as_ref().is_none_or(|g| g[r]);
        if !window_open {
            filtered_bars += 1;
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
            let (p_long, avg, atr_entry, stop_override, trail_px) = {
                let p = assets[ai].pos.as_ref().unwrap();
                (p.long, p.avg_price(), p.atr_entry, p.stop_override, p.trail_px)
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
            // The trailing level (ratcheted at the *previous* closed candle, never at this one)
            // competes with the fixed stop: the tighter of the two is what the market reaches
            // first, and it is the one that names the exit reason.
            let (sl_px, sl_reason) = match (sl_px, trail_px) {
                (Some(fixed), Some(tr)) => {
                    if if p_long { tr > fixed } else { tr < fixed } {
                        (Some(tr), "trailing_stop")
                    } else {
                        (Some(fixed), "stop_loss")
                    }
                }
                (Some(fixed), None) => (Some(fixed), "stop_loss"),
                (None, Some(tr)) => (Some(tr), "trailing_stop"),
                (None, None) => (None, "stop_loss"),
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
            // Window closed and the user asked to be flat outside it: out at this bar's open,
            // the first price available once the session is over.
            if exit.is_none() && !window_open && flat_on_close {
                exit = Some((b.open[bar], "session_end", false));
            }
            // A stop-and-reverse *opens* a position, so it obeys the window like any entry.
            if exit.is_none() && window_open && s.stop_and_reverse && opp_fires {
                exit = Some((b.open[bar], "reverse", true));
            }
            if exit.is_none() {
                if let Some(stop) = sl_px {
                    if if p_long { b.low[bar] <= stop } else { b.high[bar] >= stop } {
                        // A stop is a MARKET order once touched, so it fills at the stop price
                        // only if that price was actually available. When the bar opens beyond
                        // the stop (a gap), the open is the first price that traded and the
                        // fill can be no better than it. Filling at the stop through a gap
                        // invents liquidity and understates exactly the tail losses a stop is
                        // there to leave you exposed to.
                        let px = if p_long { stop.min(b.open[bar]) } else { stop.max(b.open[bar]) };
                        exit = Some((px, sl_reason, false));
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
                // The reverse entry sizes like a fresh one: risk-based sizing needs the
                // reverse side's stop price (from its SL rule + this bar's ATR), and equity
                // is marked-to-market across whatever other positions remain open.
                if reverse && !is_last_bar {
                    let rlong = !p.long;
                    let stop_px = first_stop_px(s, rlong, reverse_atr, b, bar, &fill);
                    let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
                    let equity_now = mtm_equity(cash, &assets, mult);
                    let margin_free = equity_now - committed_margin(&assets, mult, lev);
                    let ctx = SizeCtx {
                        equity: equity_now,
                        entry_px: fill.entry(b.open[bar], rlong),
                        stop_px,
                        leverage: s.leverage,
                        mult,
                        closed: &closed,
                    };
                    if let Some(lot) = make_lot(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, rlong, 1.0, margin_free, &ctx) {
                        assets[ai].pos = Some(Pos::new(rlong, lot, bar, reverse_atr, b.open[bar]));
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

                // Sizing/limits evaluate against marked-to-market equity, and the margin
                // check against equity net of margin already committed by open positions —
                // recomputed per asset so earlier entries on this row are accounted for.
                let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
                let equity_now = mtm_equity(cash, &assets, mult);
                let margin_free = equity_now - committed_margin(&assets, mult, lev);

                match &assets[ai].pos {
                    None => {
                        if !window_open {
                            continue;
                        }
                        let long_fired = fired(&assets[ai].long.entry, bar);
                        let short_fired = fired(&assets[ai].short.entry, bar);
                        let want_long = if long_fired { Some(true) } else if short_fired { Some(false) } else { None };
                        let Some(long) = want_long else { continue };
                        if !limits_allow_open(s, &assets, ai, b, bar, long, &fill, mult, equity_now, false) {
                            continue;
                        }
                        let sig = if long { &assets[ai].long } else { &assets[ai].short };
                        let entry_atr = sig.atr.as_ref().and_then(|v| v[bar]);
                        let stop_px = first_stop_px(s, long, entry_atr, b, bar, &fill);
                        let ctx = SizeCtx { equity: equity_now, entry_px: fill.entry(b.open[bar], long), stop_px, leverage: s.leverage, mult, closed: &closed };
                        // scale[0] for the first lot.
                        let scale0 = s.pyramid_steps.scale.first().copied().unwrap_or(1.0);
                        match make_lot_checked(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, long, scale0, margin_free, &ctx) {
                            LotOutcome::Ok(lot) => {
                                if !exposure_allows_lot(s, &assets, ai, &lot, mult, equity_now) {
                                    skipped_exposure += 1;
                                    continue;
                                }
                                assets[ai].pos = Some(Pos::new(long, lot, bar, entry_atr, b.open[bar]));
                            }
                            LotOutcome::BelowMin => skipped_min_size += 1,
                            LotOutcome::Margin => skipped_margin += 1,
                            LotOutcome::Zero => {}
                        }
                    }
                    Some(p) => {
                        if !window_open && block_adds {
                            continue;
                        }
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
                        if !limits_allow_open(s, &assets, ai, b, bar, long, &fill, mult, equity_now, true) {
                            continue;
                        }
                        let entry_atr = p.atr_entry;
                        let stop_px = first_stop_px(s, long, entry_atr, b, bar, &fill);
                        let ctx = SizeCtx { equity: equity_now, entry_px: fill.entry(b.open[bar], long), stop_px, leverage: s.leverage, mult, closed: &closed };
                        // scale[k] for add index k (clamp to last factor beyond the list).
                        let scale_k = step_scale(&s.pyramid_steps.scale, n_lots);
                        match make_lot_checked(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, long, scale_k, margin_free, &ctx) {
                            LotOutcome::Ok(lot) => {
                                if !exposure_allows_lot(s, &assets, ai, &lot, mult, equity_now) {
                                    skipped_exposure += 1;
                                    continue;
                                }
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

        // ── (3) Trailing stops ratchet, on the closed candle only ──
        // Deliberately after the entry pass and after every exit: the level a position is tested
        // against at row r was computed at row r-1 or earlier, so it can never be derived from
        // the bar that triggers it. Running it here (rather than at the top of the manage pass)
        // is also what lets a position opened at this bar's open take its first level from this
        // same candle, once that candle is actually closed.
        for a in assets.iter_mut() {
            let Some(bar) = a.bar_at_row[r] else { continue };
            let Some((long, avg)) = a.pos.as_ref().map(|p| (p.long, p.avg_price())) else {
                continue;
            };
            let rule = match if long { &a.long.trail } else { &a.short.trail } {
                Some(t) => t.clone(),
                None => continue,
            };
            let b = a.b;
            let favorable = if long { b.high[bar] } else { b.low[bar] };
            // How far in profit the candle went, against the average entry (pyramiding moves it).
            let moved = if avg > 0.0 {
                if long { (favorable - avg) / avg } else { (avg - favorable) / avg }
            } else {
                0.0
            };
            let tighten = |cur: Option<f64>, lvl: f64| {
                Some(match cur {
                    Some(x) => if long { x.max(lvl) } else { x.min(lvl) },
                    None => lvl,
                })
            };
            let mut level: Option<f64> = None;
            // Breakeven step: at `breakeven_pct` in profit the stop moves to the average entry.
            if rule.breakeven_pct > 0.0 && moved >= rule.breakeven_pct {
                level = tighten(level, avg);
            }
            // The trail proper. With no activation threshold it is live from the entry bar, so
            // the anchor is floored at the average entry: an entry candle that only went against
            // the trade still places the stop at `entry - distance`, the way the order would
            // have been placed on the exchange.
            if rule.activate_pct <= 0.0 || moved >= rule.activate_pct {
                let anchor = if rule.activate_pct <= 0.0 {
                    if long { favorable.max(avg) } else { favorable.min(avg) }
                } else {
                    favorable
                };
                if let Some(d) = rule.distance(anchor) {
                    level = tighten(level, if long { anchor - d } else { anchor + d });
                }
            }
            let Some(level) = level else { continue };
            let p = a.pos.as_mut().unwrap();
            p.trail_px = tighten(p.trail_px, level);
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
        skipped_exposure,
        halted_bars,
        filtered_bars,
        oos,
        benchmark,
        total_funding,
        grid: None,
        dca: None,
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
        ..GridConfig::default()
    });

    let levels = cfg.levels.clamp(2, 200);
    let (lo, hi) = (cfg.lower.min(cfg.upper), cfg.lower.max(cfg.upper));
    let step = if levels > 1 { (hi - lo) / (levels - 1) as f64 } else { 0.0 };
    let is_short = cfg.direction == "short";
    let cells = levels.saturating_sub(1).max(1);

    // The ladder as (bottom line, spacing) per bar. A fixed grid repeats the same pair on every
    // bar; an anchored grid re-centres it on the reference line, and stays None while the
    // reference (or the ATR backing its width) is still warming up.
    let anchored = !cfg.anchor.is_empty() && cfg.anchor != "none";
    let (mut lo_at, mut step_at) = (vec![Some(lo); n], vec![step; n]);
    if anchored {
        let p = cfg.anchor_period.clamp(1, 5_000);
        let center = match cfg.anchor.as_str() {
            "sma" => indicators::sma(b.close, p),
            "dema" => indicators::dema(b.close, p),
            "tema" => indicators::tema(b.close, p),
            "wma" => indicators::wma(b.close, p),
            "hma" => indicators::hma(b.close, p),
            "vwap" => indicators::vwap(b.high, b.low, b.close, b.volume, p),
            _ => indicators::ema(b.close, p),
        };
        let atr = (cfg.width_kind == "atr")
            .then(|| indicators::atr(b.high, b.low, b.close, cfg.width_period.clamp(1, 5_000)));
        // Bar i trades the ladder derived from bar i-1: the reference closes with the bar, so a
        // grid placed from bar i's own close could not have been filled during bar i. Same
        // no-lookahead rule the signal engine follows by filling at the next bar's open.
        for i in 0..n {
            let src = match i.checked_sub(1) {
                Some(prev) => prev,
                None => {
                    lo_at[i] = None;
                    step_at[i] = 0.0;
                    continue;
                }
            };
            let half = match (&atr, center[src]) {
                (Some(a), _) => a[src].map(|v| v * cfg.width_value),
                (None, Some(c)) => Some(c.abs() * cfg.width_value / 100.0),
                (None, None) => None,
            };
            lo_at[i] = match (center[src], half) {
                (Some(c), Some(h)) if h > 0.0 => Some(c - h),
                _ => None,
            };
            step_at[i] = half.map_or(0.0, |h| 2.0 * h / (levels - 1) as f64);
        }
    }
    // Price of grid line `k` on bar `i` (bottom line = 0).
    let line_at = |i: usize, k: usize| lo_at[i].map(|l| l + step_at[i] * k as f64);
    let warmup = lo_at.iter().position(|l| l.is_some()).unwrap_or(n);

    // Per-cell qty: fixed, or budget split across cells at the ladder's mid price. Sized once,
    // on the first tradable ladder, so every cell keeps one comparable size.
    let mid = match (lo_at.get(warmup).copied().flatten(), step_at.get(warmup)) {
        (Some(l), Some(st)) => l + st * (levels - 1) as f64 / 2.0,
        _ => (lo + hi) / 2.0,
    };
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
    // Open lot per cell: Some((entry_bar, entry_raw, target)) when that cell holds inventory.
    // Both prices travel with the lot instead of being re-read from the ladder: under an
    // anchored grid the line it was bought at is gone by the time it sells, and a lot must
    // still exit one grid step away from its own entry, which is what makes it a grid. On a
    // fixed ladder the target is exactly the cell's opposite line, so nothing changes there.
    let mut held: Vec<Option<(usize, f64, f64)>> = vec![None; cells];
    let mut fills = 0usize;
    let mut round_trips = 0usize;
    let mut halted = false;
    // Same trading-window gate as the signal engine: a closed window stops the ladder from
    // *opening* cells, while every open cell keeps its own take-profit.
    let gate = clock_gate(&s.filters, b.ts);
    let flat_on_close = s.filters.flat_on_close();
    let mut filtered_bars = 0usize;

    // A grid works with resting limit orders: both legs of a cell fill AT their line, so they
    // cross no spread and take no slippage. Only a forced exit is a market order (the
    // circuit-stop liquidation, and the close of a `reset_on_close` bar), and that one pays
    // the spread like any market fill.
    let buy_px = |raw: f64| raw * (1.0 + half_spread);
    let sell_px = |raw: f64| raw * (1.0 - half_spread);

    // Settle one completed cell (entry line → exit line) as a trade. Long: buy at entry_raw,
    // sell at exit_raw; short: sell at entry_raw, buy back at exit_raw. The entry fee was
    // already deducted from cash at fill time, so cash only settles gross − exit_fee here,
    // while the *trade* carries both fees so its pnl/fees reconcile with the equity curve.
    let settle = |trades: &mut Vec<Trade>,
                  cash: &mut f64,
                  entry_bar: usize,
                  exit_bar: usize,
                  entry_raw: f64,
                  exit_raw: f64,
                  market_exit: bool,
                  reason: &str| {
        let entry_fill = entry_raw; // limit fill, at the line
        let exit_fill = match (market_exit, is_short) {
            (false, _) => exit_raw,
            (true, true) => buy_px(exit_raw),
            (true, false) => sell_px(exit_raw),
        };
        let dir = if is_short { -1.0 } else { 1.0 };
        let gross = dir * qty * (exit_fill - entry_fill) * mult;
        let entry_fee = fee_for(&s.fees, qty, entry_fill, mult);
        let exit_fee = fee_for(&s.fees, qty, exit_fill, mult);
        let net = gross - entry_fee - exit_fee;
        *cash += gross - exit_fee; // entry fee already taken from cash at fill
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
            fees: entry_fee + exit_fee,
            return_pct: if notional != 0.0 { net / notional * 100.0 } else { 0.0 },
            bars_held: exit_bar.saturating_sub(entry_bar),
            mae: 0.0,
            mfe: 0.0,
        });
    };

    for i in 0..n {
        let window_open = gate.as_ref().is_none_or(|g| g[i]);
        if !window_open {
            filtered_bars += 1;
        }
        // No ladder yet (anchor warming up) ⇒ nothing to fill on this bar.
        if !halted && lo_at[i].is_some() {
            // Circuit stops: liquidate all inventory at this bar's open and halt.
            let breach = (cfg.stop_below > 0.0 && b.low[i] <= cfg.stop_below)
                || (cfg.stop_above > 0.0 && b.high[i] >= cfg.stop_above);
            if breach {
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, b.close[i], true, "stop_loss");
                    }
                }
                halted = true;
            } else {
                // For each cell, entry when the bar reaches the buy level, exit one step away.
                for c in 0..cells {
                    match held[c] {
                        None => {
                            if !window_open {
                                continue;
                            }
                            // Long: buy at the lower line of the cell. Short: sell at the upper one.
                            let Some(entry_line) = line_at(i, if is_short { c + 1 } else { c }) else {
                                continue;
                            };
                            let hit =
                                if is_short { b.high[i] >= entry_line } else { b.low[i] <= entry_line };
                            if hit {
                                let step_i = step_at[i];
                                let target =
                                    if is_short { entry_line - step_i } else { entry_line + step_i };
                                held[c] = Some((i, entry_line, target));
                                fills += 1;
                                // Entry fee taken on fill, on the limit price.
                                cash -= fee_for(&s.fees, qty, entry_line, mult);
                            }
                        }
                        Some((entry_bar, entry_raw, target)) => {
                            // Exit fills when price reaches the lot's own target → round trip.
                            let hit = if is_short { b.low[i] <= target } else { b.high[i] >= target };
                            if hit {
                                held[c] = None;
                                settle(&mut trades, &mut cash, entry_bar, i, entry_raw, target, false, "take_profit");
                                fills += 1;
                                round_trips += 1;
                            }
                        }
                    }
                }
            }

            // Window closed with "flat": liquidate the inventory at this bar's open, like the
            // signal engine's session_end exit.
            if !window_open && flat_on_close && !halted {
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, b.open[i], true, "session_end");
                        fills += 1;
                    }
                }
            }

            // Retrigger: flatten on the close so the next bar trades a ladder centred on the new
            // anchor. Settled cells count as fills, not as round trips: they are flushed, not
            // taken profit on.
            if cfg.reset_on_close && !halted {
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, b.close[i], true, "grid_reset");
                        fills += 1;
                    }
                }
            }
        }

        // Mark-to-market: cash + open inventory valued at this close.
        let mut inv_units = 0.0;
        let mut inv_val = 0.0;
        for c in 0..cells {
            if let Some((_, entry_raw, _)) = held[c] {
                let dir = if is_short { -1.0 } else { 1.0 };
                inv_units += dir * qty;
                inv_val += dir * qty * (b.close[i] - entry_raw) * mult;
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
            if let Some((_, entry_raw, _)) = held[c] {
                v += dir * qty * (b.close[n - 1] - entry_raw) * mult;
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
        warmup_bars: warmup,
        trading_start_ts: b.ts.get(warmup).or_else(|| b.ts.first()).cloned(),
        alignment: None,
        skipped_min_size: 0,
        skipped_margin: 0,
        skipped_exposure: 0,
        halted_bars: if halted { 1 } else { 0 },
        filtered_bars,
        oos: None,
        benchmark,
        total_funding: 0.0,
        dca: None,
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
    let side = (if long { s.long.as_ref() } else { s.short.as_ref() })?;
    let entry = fill.entry(b.open[bar], long);
    // The fixed stop is the sizing reference whenever there is one. Failing that, a trailing
    // stop that is live from the entry bar places its first level at `entry ∓ distance`, which
    // is a real stop price to size a risk against; one waiting on `activate_pct` is not.
    let dist = match side.sl_rule() {
        Some(rule) => stop_distance(&rule, entry, entry_atr)?,
        None => side.trail_at_entry()?.distance(entry)?,
    };
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
/// margin check: required margin (notional / leverage) must fit in `margin_free` — the
/// marked-to-market equity net of margin already committed by open positions.
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
    margin_free: f64,
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
    // Margin: notional / leverage must fit in the free margin (guards over-allocation,
    // including across simultaneously open positions).
    let lev = if ctx.leverage > 0.0 { ctx.leverage } else { 1.0 };
    let notional = q * px * mult;
    if notional / lev > margin_free + 1e-9 {
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
    margin_free: f64,
    ctx: &SizeCtx,
) -> Option<Lot> {
    match make_lot_checked(sizing, fees, fill, inst, mult, b, bar, long, scale, margin_free, ctx) {
        LotOutcome::Ok(lot) => Some(lot),
        _ => None,
    }
}

/// Shallow copy of a SizeCtx (its only borrow is `closed`), so callers can override entry_px.
fn reborrow_ctx<'a>(c: &SizeCtx<'a>) -> SizeCtx<'a> {
    SizeCtx {
        equity: c.equity,
        entry_px: c.entry_px,
        stop_px: c.stop_px,
        leverage: c.leverage,
        mult: c.mult,
        closed: c.closed,
    }
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
    // Attribute a trade to the segment it CLOSED in, not the one it opened in. PnL is realized
    // at the exit, and the equity curve (partitioned just above on bar timestamp) moves there
    // too, so exit-time bucketing is the only rule under which a segment's `net_pnl` and its
    // equity slice agree. Bucketing by entry time put a straddling trade's whole PnL in-sample
    // while its equity move landed out-of-sample, which reported ~0 OOS return over a segment
    // where equity demonstrably moved — the most misleading possible direction for the one
    // statistic that exists to detect overfitting.
    let is_trades: Vec<&Trade> = trades.iter().filter(|t| t.exit_ts < split_ts).collect();
    let oos_trades: Vec<&Trade> = trades.iter().filter(|t| t.exit_ts >= split_ts).collect();

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
    // Exposure caps are re-checked against the *sized* lot in `exposure_allows_lot` once the
    // quantity is known. This pass only rejects a book that is already over the cap, which
    // saves sizing work; it can never be the whole check, because the lot about to fill is
    // exactly what pushes the book over.
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

/// Whether the exposure caps still hold once `lot` is added to the book.
///
/// The caps bound the notional the portfolio *ends up* carrying, so the lot being opened has to
/// count toward them. Checking only what is already open (which is all `limits_allow_open` can
/// do, before sizing) lets every entry through as long as the book was under the cap *before*
/// it, so the book lands one whole position over the limit.
fn exposure_allows_lot(s: &Settings, assets: &[Asset], ai: usize, lot: &Lot, mult: f64, equity: f64) -> bool {
    let risk = &s.risk;
    let add = lot.qty.abs() * lot.price.abs() * mult;
    if let Some(cap) = risk.max_exposure_pct {
        if cap > 0.0 {
            let open: f64 = assets.iter().filter_map(|a| a.pos.as_ref().map(|p| p.notional(a.last_close, mult))).sum();
            if open + add > equity * cap / 100.0 + 1e-9 {
                return false;
            }
        }
    }
    if let Some(cap) = risk.max_exposure_per_asset_pct {
        if cap > 0.0 {
            let this = assets[ai].pos.as_ref().map(|p| p.notional(lot.price, mult)).unwrap_or(0.0);
            if this + add > equity * cap / 100.0 + 1e-9 {
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
    pub(super) fn cond(l: f64, r: f64) -> Signal {
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
            dca: None,
            mode: Mode::Long,
            long: Some(Side {
                signal: None,
                entry: Some(entry),
                exit: None,
                stop_loss_pct: 0.0,
                take_profit_pct: 0.0,
                stop_loss: None,
                take_profit: None,
                trailing_stop: None,
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
            filters: Filters::default(),
        }
    }



    /// Audit helper: build OHLC Bars from slices.
    pub(super) fn tests_bars<'a>(
        ts: &'a [String],
        o: &'a [f64],
        h: &'a [f64],
        l: &'a [f64],
        c: &'a [f64],
    ) -> Bars<'a> {
        Bars { ticker: "X", ts, open: o, high: h, low: l, close: c, volume: c }
    }

    /// Audit helpers (used by the `verify` module): an always-firing long / short setup.
    pub(super) fn settings_always_long() -> Settings {
        base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] })
    }
    pub(super) fn settings_always_short() -> Settings {
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.mode = Mode::Short;
        s.short = s.long.take();
        s
    }

    /// The configuration that silently turns a trend strategy into a one-bar shuffle: a
    /// crossover entry, `exit_on_reverse`, and no exit condition. It runs and it looks fine,
    /// so the only defence is saying so in the response.
    #[test]
    fn crossover_with_exit_on_reverse_and_no_exit_warns() {
        let cross = |op: Op| SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Const { value: 1.0 },
                op,
                right: Some(Operand::Const { value: 2.0 }),
            }],
        };
        let mut s = base_settings(cross(Op::CrossesAbove));
        s.long.as_mut().unwrap().exit_on_reverse = true;
        let w = s.warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("avg_bars_held"), "{w:?}");

        // An explicit exit is the fix, and it silences the warning.
        s.long.as_mut().unwrap().exit = Some(cross(Op::CrossesBelow));
        assert!(s.warnings().is_empty());

        // A state entry means "while true", so exiting when it stops holding is the point.
        let mut state = base_settings(cross(Op::Above));
        state.long.as_mut().unwrap().exit_on_reverse = true;
        assert!(state.warnings().is_empty());

        // A side that cannot trade cannot mislead: mode long, short block ignored.
        let mut short_only = base_settings(cross(Op::CrossesAbove));
        short_only.short = short_only.long.clone();
        short_only.short.as_mut().unwrap().exit_on_reverse = true;
        assert!(short_only.warnings().is_empty());
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
    fn fixed_qty_scales_with_leverage() {
        let (ts, px) = flat_bars(10);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.leverage = 10.0;
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.trades[0].qty, 10.0);
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
    ///
    /// Re-baselined at ENGINE_VERSION 3 (was 9071.235667736271 at v2). The move is entirely the
    /// gapped-stop fix: a stop touched by a bar that OPENED beyond it now fills at that open
    /// instead of at the stop price, so this fixture's stopped-out trades realize their true
    /// (worse) loss. Verified by reverting that one change, which restores the v2 number exactly.
    const V1_FINAL_EQUITY: f64 = 8870.847780847464;

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

    /// Two providers, one market: a 24/7 series stamped at the candle open (00:00 UTC) and an
    /// exchange series stamped at the session start (05:00 UTC). These are the same six days,
    /// and the merged clock must say so: before the shared aligner the two never shared a row,
    /// the clock ran twice as long and the overlap was empty.
    #[test]
    fn portfolio_aligns_two_stamping_conventions_on_one_clock() {
        let n = 6;
        let ts_a: Vec<String> =
            (0..n).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let ts_b: Vec<String> =
            (0..n).map(|i| format!("2020-01-{:02}T05:00:00Z", i + 1)).collect();
        let pa: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let pb: Vec<f64> = (0..n).map(|i| 200.0 - i as f64).collect();
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let r = run_portfolio(&s, &[&tk_bars("BTC", &ts_a, &pa), &tk_bars("SPY", &ts_b, &pb)]);
        let al = r.alignment.unwrap();
        assert_eq!(al.clock_len, 6, "one row per day, not one per stamping convention");
        assert_eq!(al.overlap_bars, 6, "the two assets coexist on every row");
        assert_eq!(al.grain.as_deref(), Some("1d"));
        assert!(al.assets.iter().all(|a| a.inactive_bars == 0));
        assert_eq!(r.equity.len(), 6);
        // The row is stamped with the latest real bar it holds, never an invented midnight.
        assert_eq!(r.equity[0].ts, "2020-01-01T05:00:00Z");
    }

    /// An asset with two bars inside one day must not lose one to bucketing: the merge falls
    /// back to exact timestamps and says so (no granularity in the report).
    #[test]
    fn portfolio_keeps_every_bar_when_two_share_a_period() {
        let ts_a: Vec<String> = vec![
            "2020-01-01T00:00:00Z".into(),
            "2020-01-02T00:00:00Z".into(),
            "2020-01-03T00:00:00Z".into(),
            "2020-01-03T12:00:00Z".into(),
            "2020-01-04T00:00:00Z".into(),
            "2020-01-05T00:00:00Z".into(),
            "2020-01-06T00:00:00Z".into(),
        ];
        let ts_b: Vec<String> =
            (0..6).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let pa = vec![100.0; ts_a.len()];
        let pb = vec![50.0; ts_b.len()];
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let r = run_portfolio(&s, &[&tk_bars("AAA", &ts_a, &pa), &tk_bars("BBB", &ts_b, &pb)]);
        let al = r.alignment.unwrap();
        assert_eq!(al.clock_len, 7, "the extra intraday bar keeps its own row");
        assert_eq!(al.grain, None, "bucketing would have dropped a bar, so it was not used");
        let aaa = al.assets.iter().find(|x| x.ticker == "AAA").unwrap();
        assert_eq!(aaa.bars, 7);
        assert_eq!(aaa.inactive_bars, 0);
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

    /// Stop-and-reverse must open the reverse position under risk-based sizing too: the
    /// reverse entry needs a stop price to size against (regression — it used to be
    /// silently skipped, leaving SAR+risk runs long-only).
    #[test]
    fn stop_and_reverse_opens_under_risk_sizing() {
        let n = 12;
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        // Flat at 100, then a step down to 90 — the short entry (close < 95) fires there.
        let px: Vec<f64> = (0..n).map(|i| if i < 5 { 100.0 } else { 90.0 }).collect();
        let long_entry = SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] };
        let short_entry = SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Price { field: "close".into() },
                op: Op::Below,
                right: Some(Operand::Const { value: 95.0 }),
            }],
        };
        let mut s = base_settings(long_entry);
        s.mode = Mode::Both;
        s.stop_and_reverse = true;
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.5; // wide: never hit by the 10% step
        s.short = Some(Side {
            signal: None,
            entry: Some(short_entry),
            exit: None,
            stop_loss_pct: 0.5,
            take_profit_pct: 0.0,
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
            exit_on_reverse: false,
        });
        let r = run(&s, &bars(&ts, &px));
        assert!(
            r.trades.iter().any(|t| t.exit_reason == "reverse"),
            "expected a reverse exit"
        );
        assert!(
            r.trades.iter().any(|t| t.direction == "short"),
            "reverse entry should open a short position under risk sizing"
        );
    }

    /// The margin check runs against equity net of margin already committed: two assets
    /// whose fixed size each consumes the whole account can't both open at 1× leverage.
    #[test]
    fn margin_accounts_for_open_positions() {
        let (ts, px) = flat_bars(8);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        // 100 units at price 100 = 10k notional = the entire starting capital.
        s.sizing = Sizing::FixedQty { qty: 100.0 };
        let a = tk_bars("AAA", &ts, &px);
        let b = tk_bars("BBB", &ts, &px);
        let r = run_portfolio(&s, &[&a, &b]);
        assert!(r.skipped_margin > 0, "second asset should be refused on margin");
        let traded: Vec<&AssetStats> = r.per_asset.iter().filter(|x| x.trades > 0).collect();
        assert_eq!(traded.len(), 1, "only one asset can fund a full-size position");
    }

    /// Grid trades carry the entry fee: Σ trade.pnl must reconcile with the cash-based
    /// net (final equity − start − open inventory value).
    #[test]
    fn grid_trade_pnl_includes_entry_fee() {
        let n = 40;
        let ts: Vec<String> = (0..n).map(|i| format!("t{i:02}")).collect();
        // Oscillate across [90, 110] so cells complete round trips.
        let px: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 90.0 } else { 110.0 }).collect();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![] });
        s.kind = "grid".into();
        s.grid = Some(GridConfig {
            lower: 90.0,
            upper: 110.0,
            levels: 5,
            qty_per_level: 1.0,
            ..GridConfig::default()
        });
        s.fees = Fees { amount_kind: "fixed".into(), per: "trade".into(), amount: 1.0 };
        let r = run(&s, &bars(&ts, &px));
        assert!(!r.trades.is_empty());
        // Every completed round trip pays 2 × the $1 flat fee.
        for t in &r.trades {
            approx(t.fees, 2.0);
        }
        // Trade PnL reconciles with the equity curve: closed pnl + open-inventory entry
        // fees (already spent, not yet realized) = final equity − start − inventory value.
        let g = r.grid.as_ref().unwrap();
        let open_cells = (g.end_inventory / 1.0).round();
        let closed_pnl: f64 = r.trades.iter().map(|t| t.pnl).sum();
        let cash_net = r.stats.final_equity - s.starting_capital - g.end_inventory_value;
        approx(closed_pnl - open_cells * 1.0, cash_net);
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

    /// Hourly bars over one UTC day, always-true entry. Helper for the filter tests.
    fn day_of_hours(day: &str) -> (Vec<String>, Vec<f64>) {
        let ts: Vec<String> = (0..24).map(|i| format!("{day}T{i:02}:00:00Z")).collect();
        (ts, vec![100.0; 24])
    }

    /// Session filter: the first fill lands on the first bar inside the window, and every row
    /// outside it is counted.
    #[test]
    fn session_filter_gates_entries() {
        let (ts, px) = day_of_hours("2020-06-01");
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.sessions = vec![Session { from: "09:00".into(), to: "12:00".into() }];
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entry_ts, "2020-06-01T09:00:00Z");
        assert_eq!(r.filtered_bars, 21); // 24 rows, 09/10/11 open
    }

    /// The session is read on the filters' local clock: UTC-5 shifts the same window by 5 hours.
    #[test]
    fn session_filter_honours_utc_offset() {
        let (ts, px) = day_of_hours("2020-06-01");
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.sessions = vec![Session { from: "09:00".into(), to: "12:00".into() }];
        s.filters.tz_offset_min = -300;
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entry_ts, "2020-06-01T14:00:00Z");
    }

    /// A window ending with `flat` closes the position at the open of the first bar outside it.
    #[test]
    fn session_filter_flat_at_window_end() {
        let (ts, px) = day_of_hours("2020-06-01");
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.sessions = vec![Session { from: "09:00".into(), to: "12:00".into() }];
        s.filters.on_window_end = "flat".into();
        let r = run(&s, &bars(&ts, &px));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "session_end");
        assert_eq!(t.exit_ts, "2020-06-01T12:00:00Z");
    }

    /// A window whose end is before its start wraps past midnight.
    #[test]
    fn session_filter_wraps_midnight() {
        let (ts, px) = day_of_hours("2020-06-01");
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.sessions = vec![Session { from: "22:00".into(), to: "02:00".into() }];
        let r = run(&s, &bars(&ts, &px));
        // 00:00 and 01:00 are inside; the fill needs a prior bar, so the first is 01:00.
        assert_eq!(r.trades[0].entry_ts, "2020-06-01T01:00:00Z");
        assert_eq!(r.filtered_bars, 20); // 00, 01, 22, 23 open
    }

    /// Weekday filter: 2020-06-01 is a Monday, so a Sat/Sun rule waits for the 6th.
    #[test]
    fn weekday_filter_gates_entries() {
        let ts: Vec<String> = (1..=14).map(|d| format!("2020-06-{d:02}T00:00:00Z")).collect();
        let px = vec![100.0; ts.len()];
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.weekdays = vec![6, 7];
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entry_ts, "2020-06-06T00:00:00Z");
    }

    /// Date rules: an excluded range pushes the first entry past it, an include list pins it.
    #[test]
    fn date_filters_gate_entries() {
        let ts: Vec<String> = (1..=14).map(|d| format!("2020-06-{d:02}T00:00:00Z")).collect();
        let px = vec![100.0; ts.len()];
        let entry = || SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] };

        let mut s = base_settings(entry());
        s.filters.exclude_dates =
            vec![DateRange { from: "2020-06-01".into(), to: Some("2020-06-05".into()) }];
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entry_ts, "2020-06-06T00:00:00Z");

        let mut s = base_settings(entry());
        s.filters.include_dates = vec![DateRange { from: "2020-06-09".into(), to: None }];
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entry_ts, "2020-06-09T00:00:00Z");
        assert_eq!(r.filtered_bars, 13);
    }

    /// `block_adds` off lets a position keep building outside the window (it never opens one).
    #[test]
    fn filters_can_let_adds_through() {
        let (ts, px) = day_of_hours("2020-06-01");
        let entry = || SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] };
        let mut s = base_settings(entry());
        s.pyramiding = 3;
        s.filters.sessions = vec![Session { from: "09:00".into(), to: "11:00".into() }];
        let blocked = run(&s, &bars(&ts, &px));
        s.filters.block_adds = false;
        let allowed = run(&s, &bars(&ts, &px));
        assert_eq!(blocked.trades[0].entries, 2); // 09:00 + 10:00, then the window closes
        assert_eq!(allowed.trades[0].entries, 3);
    }

    /// Inert filters are exactly that: no gate is built, so the hot path is untouched.
    #[test]
    fn empty_filters_are_inert() {
        let f = Filters::default();
        assert!(!f.is_active());
        assert!(clock_gate(&f, &["2020-06-01T00:00:00Z".to_string()]).is_none());
        assert!(f.validate().is_none());
    }

    /// Malformed rules are refused before the run rather than quietly doing nothing.
    #[test]
    fn filter_validation_rejects_nonsense() {
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.weekdays = vec![8];
        assert!(s.validate().is_some());

        s.filters = Filters { sessions: vec![Session { from: "9h".into(), to: "12:00".into() }], ..Default::default() };
        assert!(s.validate().is_some());

        s.filters = Filters { sessions: vec![Session { from: "09:00".into(), to: "09:00".into() }], ..Default::default() };
        assert!(s.validate().is_some());

        s.filters = Filters { exclude_dates: vec![DateRange { from: "01/06/2020".into(), to: None }], ..Default::default() };
        assert!(s.validate().is_some());

        s.filters = Filters { on_window_end: "close".into(), ..Default::default() };
        assert!(s.validate().is_some());
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
            ..GridConfig::default()
        });
        let r = run(&s, &ohlc(&ts, &c, &h, &l, &c));
        let g = r.grid.expect("grid stats present");
        assert!(g.round_trips > 0, "expected grid round trips, got {}", g.round_trips);
        assert_eq!(g.levels, 5);
        // Completed round trips are recorded as trades.
        assert_eq!(r.stats.trades, g.round_trips);
    }

    /// An anchored grid follows the reference line: a price that drifts away from its starting
    /// band keeps trading, where the same ladder pinned to that band goes quiet after the drift.
    #[test]
    fn grid_anchored_follows_price() {
        // Oscillation around a rising trend: leaves [90, 110] for good after ~30 bars.
        let n = 200;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-01T{:02}:00:00Z", i % 24)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.5 + 4.0 * ((i as f64) * 0.7).sin()).collect();
        let h: Vec<f64> = c.iter().map(|x| x + 0.5).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 0.5).collect();
        let bars = ohlc(&ts, &c, &h, &l, &c);

        let mut fixed = base_settings(SignalGroup { logic: "all".into(), conditions: vec![] });
        fixed.kind = "grid".into();
        fixed.grid = Some(GridConfig { lower: 90.0, upper: 110.0, levels: 5, qty_per_level: 1.0, ..GridConfig::default() });
        let r_fixed = run(&fixed, &bars);

        let mut anchored = fixed.clone();
        anchored.grid = Some(GridConfig {
            levels: 5,
            qty_per_level: 1.0,
            anchor: "ema".into(),
            anchor_period: 20,
            width_kind: "pct".into(),
            width_value: 3.0,
            ..GridConfig::default()
        });
        let r_anch = run(&anchored, &bars);

        let (g_fixed, g_anch) = (r_fixed.grid.unwrap(), r_anch.grid.unwrap());
        assert!(
            g_anch.round_trips > g_fixed.round_trips,
            "anchored {} should out-trade fixed {}",
            g_anch.round_trips,
            g_fixed.round_trips
        );
        // The ladder only exists once the EMA does; those bars are the warm-up.
        assert!(r_anch.warmup_bars > 0, "anchored grid should report a warm-up");
        assert_eq!(r_fixed.warmup_bars, 0);
        // Round trips are still profitable per cell (bought low, sold one line higher).
        assert!(r_anch.trades.iter().filter(|t| t.exit_reason == "take_profit").all(|t| t.pnl > 0.0));
    }

    /// `reset_on_close` flattens the book on every bar: nothing is carried, and each settled cell
    /// is a same-bar trade tagged `grid_reset`.
    #[test]
    fn grid_reset_on_close_flattens() {
        let n = 60;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-01T{:02}:00:00Z", i % 24)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 + 6.0 * ((i as f64) * 0.5).sin()).collect();
        let h: Vec<f64> = c.iter().map(|x| x + 2.0).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 2.0).collect();
        let bars = ohlc(&ts, &c, &h, &l, &c);

        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![] });
        s.kind = "grid".into();
        s.grid = Some(GridConfig {
            levels: 5,
            qty_per_level: 1.0,
            anchor: "sma".into(),
            anchor_period: 10,
            width_kind: "pct".into(),
            width_value: 3.0,
            reset_on_close: true,
            ..GridConfig::default()
        });
        let r = run(&s, &bars);
        let g = r.grid.expect("grid stats present");

        assert_eq!(g.end_inventory, 0.0, "no inventory may survive a close");
        assert!(r.trades.iter().any(|t| t.exit_reason == "grid_reset"), "expected reset exits");
        // Every trade opened and closed inside one bar.
        assert!(r.trades.iter().all(|t| t.bars_held == 0));
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

/// Arithmetic verification suite (audit, 2026-08-20). Every assertion here is a value computed
/// by hand from the settings, not captured from the engine, so a failure means the engine and
/// the documented semantics disagree.
#[cfg(test)]
mod verify {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }

    fn ts_of(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect()
    }

    fn mk<'a>(tk: &'a str, ts: &'a [String], o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> Bars<'a> {
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c }
    }

    // ---------------------------------------------------------------- fees

    /// fee_for: per=unit + fixed amount must be amount × qty, independent of price/multiplier.
    #[test]
    fn v_fee_per_unit_fixed() {
        let f = Fees { amount_kind: "fixed".into(), per: "unit".into(), amount: 0.5 };
        approx_v(fee_for(&f, 3.0, 100.0, 1.0), 1.5, 1e-12, "unit/fixed");
        approx_v(fee_for(&f, 3.0, 100.0, 50.0), 1.5, 1e-12, "unit/fixed ignores multiplier");
    }

    /// fee_for: per=unit + pct must charge on notional including the multiplier.
    #[test]
    fn v_fee_per_unit_pct_uses_multiplier() {
        let f = Fees { amount_kind: "pct".into(), per: "unit".into(), amount: 0.1 };
        // notional = 3 × 100 × 50 = 15000; 0.1% = 15
        approx_v(fee_for(&f, 3.0, 100.0, 50.0), 15.0, 1e-9, "unit/pct");
    }

    /// fee_for: per=trade + fixed is a flat charge whatever the size.
    #[test]
    fn v_fee_per_trade_fixed_flat() {
        let f = Fees { amount_kind: "fixed".into(), per: "trade".into(), amount: 2.0 };
        approx_v(fee_for(&f, 1.0, 100.0, 1.0), 2.0, 1e-12, "trade/fixed q=1");
        approx_v(fee_for(&f, 99.0, 100.0, 1.0), 2.0, 1e-12, "trade/fixed q=99");
    }

    // ---------------------------------------------------------------- sizing

    /// percent_equity: notional = equity × pct × leverage, qty = notional / entry price.
    /// With a contract multiplier the *notional* is qty × price × multiplier, so a correct
    /// sizing must divide by (price × multiplier) to actually deploy that much capital.
    #[test]
    fn v_sizing_percent_equity_multiplier() {
        let closed: Vec<ClosedTrade> = vec![];
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 1.0, closed: &closed };
        let q = resolve_qty(&Sizing::PercentEquity { percent: 50.0 }, &c);
        // Intent: deploy 5000 of notional. With multiplier 1 that is 50 units.
        approx_v(q, 50.0, 1e-9, "percent_equity mult=1");
    }

    /// risk sizing: qty × per-unit risk must equal risk_pct% of equity **in account currency**,
    /// which for a multiplier contract means per-unit risk = |entry-stop| × multiplier.
    #[test]
    fn v_sizing_risk_respects_multiplier() {
        // ES-like: multiplier 50, entry 100, stop 98 → per-unit risk = 2 × 50 = 100 currency.
        // Risking 1% of 10_000 = 100 currency ⇒ qty must be 1.0 contract.
        let mut s = settings_always_long();
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0 };
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.02; // stop 2% below entry
        s.starting_capital = 10_000.0;
        let n = 6;
        let ts = ts_of(n);
        let o = vec![100.0; n];
        let h = vec![100.0; n];
        let l = vec![100.0; n];
        let c = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        // Direct proof of the sizing arithmetic, independent of whether the entry survives the
        // margin check: resolve_qty must account for the multiplier.
        let closed: Vec<ClosedTrade> = vec![];
        let ctx = SizeCtx {
            equity: 10_000.0,
            entry_px: 100.0,
            stop_px: Some(98.0),
            leverage: 1.0,
            mult: 50.0,
            closed: &closed,
        };
        let q = resolve_qty(&Sizing::Risk { risk_pct: 1.0 }, &ctx);
        // Per-contract risk = |100-98| x 50 = 100 currency. Risking 1% of 10_000 = 100 ⇒ 1 contract.
        approx_v(q, 1.0, 1e-6, "risk sizing must divide by (|entry-stop| x multiplier)");
        assert!(!r.trades.is_empty(), "expected a trade (oversize was refused by margin)");
        approx_v(r.trades[0].qty, 1.0, 1e-6, "risk-sized qty with multiplier 50");
    }

    /// The same risk sizing with multiplier 1 is the uncontested baseline: |100-98| = 2 per unit,
    /// risking 100 currency ⇒ 50 units.
    #[test]
    fn v_sizing_risk_baseline_no_multiplier() {
        let mut s = settings_always_long();
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.02;
        s.starting_capital = 10_000.0;
        let n = 6;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        assert!(!r.trades.is_empty());
        approx_v(r.trades[0].qty, 50.0, 1e-6, "risk-sized qty, multiplier 1");
    }

    /// A losing trade stopped out under risk sizing must lose exactly risk_pct% of equity
    /// (gross of fees). This is the end-to-end meaning of "risk 1% per trade".
    #[test]
    fn v_risk_sizing_loss_equals_risk_budget() {
        let mut s = settings_always_long();
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.02;
        s.starting_capital = 10_000.0;
        // Bar1 fills at open 100, stop = 98. Bar2 low pierces 98 → stop fills at 98.
        let ts = ts_of(4);
        let o = vec![100.0, 100.0, 100.0, 100.0];
        let h = vec![100.0, 100.0, 100.0, 100.0];
        let l = vec![100.0, 100.0, 90.0, 90.0];
        let c = vec![100.0, 100.0, 95.0, 95.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "stop_loss").expect("a stop_loss trade");
        approx_v(t.pnl, -100.0, 1e-6, "1% of 10_000 risked");
    }

    // ---------------------------------------------------------------- pnl / multiplier

    /// Multiplier must scale PnL: 1 contract, price 100→110, multiplier 50 ⇒ 500 currency.
    #[test]
    fn v_multiplier_scales_pnl_exactly() {
        let mut s = settings_always_long();
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0 };
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.10; // TP at 110
        let ts = ts_of(4);
        let o = vec![100.0, 100.0, 100.0, 100.0];
        let h = vec![100.0, 100.0, 120.0, 120.0];
        let l = vec![100.0, 100.0, 100.0, 100.0];
        let c = vec![100.0, 100.0, 115.0, 115.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "take_profit").expect("a tp trade");
        approx_v(t.exit_price, 110.0, 1e-6, "tp price");
        approx_v(t.pnl, 500.0, 1e-6, "(110-100) x 1 x 50");
    }

    /// Short PnL sign and magnitude: sell 100, cover 90, qty 2 ⇒ +20.
    #[test]
    fn v_short_pnl_sign() {
        let mut s = settings_always_short();
        s.sizing = Sizing::FixedQty { qty: 2.0 };
        s.short.as_mut().unwrap().take_profit_pct = 0.10; // cover at 90
        let ts = ts_of(4);
        let o = vec![100.0, 100.0, 100.0, 100.0];
        let h = vec![100.0, 100.0, 100.0, 100.0];
        let l = vec![100.0, 100.0, 80.0, 80.0];
        let c = vec![100.0, 100.0, 85.0, 85.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "take_profit").expect("a tp trade");
        approx_v(t.exit_price, 90.0, 1e-6, "short tp = avg x 0.9");
        approx_v(t.pnl, 20.0, 1e-6, "(100-90) x 2");
    }

    /// return_pct must be net PnL over the entry notional, multiplier included.
    #[test]
    fn v_return_pct_uses_notional_with_multiplier() {
        let mut s = settings_always_long();
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0 };
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.10;
        let ts = ts_of(4);
        let o = vec![100.0; 4];
        let h = vec![100.0, 100.0, 120.0, 120.0];
        let l = vec![100.0; 4];
        let c = vec![100.0, 100.0, 115.0, 115.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "take_profit").unwrap();
        // notional = 100 × 1 × 50 = 5000; pnl 500 ⇒ 10%
        approx_v(t.return_pct, 10.0, 1e-6, "return_pct");
    }

    // ---------------------------------------------------------------- equity conservation

    /// The invariant that ties everything together: final_equity must equal starting capital
    /// plus the sum of every closed trade's net PnL (no position left open at the end).
    #[test]
    fn v_equity_reconciles_with_trades() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 2.0 };
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 0.05 };
        s.spread_pct = 0.02;
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.03;
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.7).sin() * 8.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 2.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 2.0).collect();
        let c: Vec<f64> = o.iter().map(|x| x + 0.3).collect();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let sum: f64 = r.trades.iter().map(|t| t.pnl).sum();
        approx_v(r.stats.final_equity, s.starting_capital + sum, 1e-6, "equity = capital + Σpnl");
        approx_v(r.stats.net_pnl, sum, 1e-6, "net_pnl = Σpnl");
    }

    /// Same invariant for a multi-asset portfolio run.
    #[test]
    fn v_portfolio_equity_reconciles() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 0.05 };
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.05;
        let n = 30;
        let ts = ts_of(n);
        let o1: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.5).sin() * 6.0).collect();
        let h1: Vec<f64> = o1.iter().map(|x| x + 3.0).collect();
        let l1: Vec<f64> = o1.iter().map(|x| x - 3.0).collect();
        let c1: Vec<f64> = o1.clone();
        let o2: Vec<f64> = (0..n).map(|i| 50.0 + (i as f64 * 0.9).cos() * 4.0).collect();
        let h2: Vec<f64> = o2.iter().map(|x| x + 2.0).collect();
        let l2: Vec<f64> = o2.iter().map(|x| x - 2.0).collect();
        let c2: Vec<f64> = o2.clone();
        let a = mk("A", &ts, &o1, &h1, &l1, &c1);
        let bb = mk("B", &ts, &o2, &h2, &l2, &c2);
        let r = run_portfolio(&s, &[&a, &bb]);
        let sum: f64 = r.trades.iter().map(|t| t.pnl).sum();
        approx_v(r.stats.final_equity, s.starting_capital + sum, 1e-6, "portfolio equity");
        assert!(r.trades.iter().any(|t| t.ticker == "A"), "A traded");
        assert!(r.trades.iter().any(|t| t.ticker == "B"), "B traded");
    }

    /// Two identical assets run as a portfolio must produce exactly twice the PnL of one asset
    /// run alone, when no portfolio limit binds and sizing is fixed qty.
    #[test]
    fn v_portfolio_two_identical_assets_double_pnl() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.05;
        let n = 30;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.5).sin() * 6.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 3.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 3.0).collect();
        let c: Vec<f64> = o.clone();
        let a = mk("A", &ts, &o, &h, &l, &c);
        let bb = mk("B", &ts, &o, &h, &l, &c);
        let single = run(&s, &a);
        let both = run_portfolio(&s, &[&a, &bb]);
        approx_v(both.stats.net_pnl, single.stats.net_pnl * 2.0, 1e-6, "2 identical assets = 2x");
    }

    // ---------------------------------------------------------------- pyramiding

    /// Weighted-average entry across three lots at distinct prices, and the resulting PnL.
    #[test]
    fn v_pyramiding_weighted_average_entry() {
        let mut s = settings_always_long();
        s.pyramiding = 3;
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        // opens: bar1=100, bar2=110, bar3=120 → three lots, avg = 110
        let ts = ts_of(6);
        let o = vec![100.0, 100.0, 110.0, 120.0, 130.0, 130.0];
        let h = o.clone();
        let l = o.clone();
        let c = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        assert_eq!(r.trades.len(), 1, "one position");
        let t = &r.trades[0];
        assert_eq!(t.entries, 3);
        approx_v(t.qty, 3.0, 1e-9, "3 lots");
        approx_v(t.entry_price, 110.0, 1e-9, "(100+110+120)/3");
        // exit at last close 130 → (130-110) × 3 = 60
        approx_v(t.pnl, 60.0, 1e-9, "weighted avg pnl");
    }

    /// Pyramiding scale sequence: qty per add = base × scale[k].
    #[test]
    fn v_pyramid_scale_quantities() {
        let mut s = settings_always_long();
        s.pyramiding = 3;
        s.sizing = Sizing::FixedQty { qty: 2.0 };
        s.pyramid_steps = PyramidSteps { scale: vec![1.0, 0.5, 0.25], ..PyramidSteps::default() };
        let ts = ts_of(6);
        let px = vec![100.0; 6];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        let t = &r.trades[0];
        // 2.0 + 1.0 + 0.5 = 3.5
        approx_v(t.qty, 3.5, 1e-9, "scaled adds");
    }

    // ---------------------------------------------------------------- leverage

    /// FixedQty × leverage: the documented retail convention is qty × leverage exposure.
    #[test]
    fn v_leverage_scales_fixed_qty() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.leverage = 3.0;
        let ts = ts_of(5);
        let px = vec![100.0; 5];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        approx_v(r.trades[0].qty, 3.0, 1e-9, "1 x 3");
    }

    /// Leverage must not change the *stop-loss risk* under risk-based sizing: risking 1% is 1%
    /// whatever the leverage (leverage only relaxes the margin ceiling).
    #[test]
    fn v_leverage_does_not_change_risk_budget() {
        let mk_s = |lev: f64| {
            let mut s = settings_always_long();
            s.sizing = Sizing::Risk { risk_pct: 1.0 };
            s.long.as_mut().unwrap().stop_loss_pct = 0.02;
            s.leverage = lev;
            s
        };
        let ts = ts_of(4);
        let o = vec![100.0; 4];
        let h = vec![100.0; 4];
        let l = vec![100.0, 100.0, 90.0, 90.0];
        let c = vec![100.0, 100.0, 95.0, 95.0];
        let b = mk("X", &ts, &o, &h, &l, &c);
        let r1 = run(&mk_s(1.0), &b);
        let r5 = run(&mk_s(5.0), &b);
        let p1 = r1.trades.iter().find(|t| t.exit_reason == "stop_loss").unwrap().pnl;
        let p5 = r5.trades.iter().find(|t| t.exit_reason == "stop_loss").unwrap().pnl;
        approx_v(p5, p1, 1e-6, "risk budget independent of leverage");
    }

    // ---------------------------------------------------------------- spread / slippage

    /// Spread: long entry pays half-spread up, long exit receives half-spread down.
    #[test]
    fn v_spread_symmetry_long_short() {
        let ts = ts_of(5);
        let px = vec![100.0; 5];
        let mut sl = settings_always_long();
        sl.spread_pct = 0.02; // fraction: 2% spread → half 1% → 100 ± 1.0
        sl.sizing = Sizing::FixedQty { qty: 1.0 };
        let rl = run(&sl, &mk("X", &ts, &px, &px, &px, &px));
        approx_v(rl.trades[0].entry_price, 101.0, 1e-9, "long entry + half spread");
        approx_v(rl.trades[0].exit_price, 99.0, 1e-9, "long exit - half spread");
        let mut ss = settings_always_short();
        ss.spread_pct = 0.02;
        ss.sizing = Sizing::FixedQty { qty: 1.0 };
        let rs = run(&ss, &mk("X", &ts, &px, &px, &px, &px));
        approx_v(rs.trades[0].entry_price, 99.0, 1e-9, "short entry - half spread");
        approx_v(rs.trades[0].exit_price, 101.0, 1e-9, "short exit + half spread");
    }

    /// Slippage in ticks worsens both legs by value × tick_size.
    #[test]
    fn v_slippage_ticks_both_legs() {
        let ts = ts_of(5);
        let px = vec![100.0; 5];
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.slippage = Slippage { kind: "ticks".into(), value: 2.0, tick_size: 0.25 }; // 0.5
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        approx_v(r.trades[0].entry_price, 100.5, 1e-9, "entry slipped up");
        approx_v(r.trades[0].exit_price, 99.5, 1e-9, "exit slipped down");
        approx_v(r.trades[0].pnl, -1.0, 1e-9, "round-trip slippage cost");
    }

    // ---------------------------------------------------------------- stops

    /// A gap through the stop must fill at the *stop price* or worse, never better. Here the bar
    /// opens far below the stop, so a realistic engine fills at the open, not at the stop.
    #[test]
    fn v_gap_through_stop_fill_price() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.05; // stop = 95
        let ts = ts_of(4);
        let o = vec![100.0, 100.0, 80.0, 80.0]; // bar2 gaps to 80, well below the 95 stop
        let h = vec![100.0, 100.0, 82.0, 82.0];
        let l = vec![100.0, 100.0, 78.0, 78.0];
        let c = vec![100.0, 100.0, 80.0, 80.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "stop_loss").expect("stopped out");
        assert!(
            t.exit_price <= 95.0 + 1e-9,
            "gap fill must not be better than the stop: got {}",
            t.exit_price
        );
        assert!(
            t.exit_price <= 80.0 + 1e-9,
            "bar opened at 80 through the stop; filling at 95 invents liquidity: got {}",
            t.exit_price
        );
    }

    /// When a single bar contains both the stop and the target, the engine must resolve it
    /// conservatively (stop first). Pin the documented behavior.
    #[test]
    fn v_stop_wins_when_bar_spans_both() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.05; // 95
        s.long.as_mut().unwrap().take_profit_pct = 0.05; // 105
        let ts = ts_of(4);
        let o = vec![100.0, 100.0, 100.0, 100.0];
        let h = vec![100.0, 100.0, 110.0, 110.0]; // hits TP
        let l = vec![100.0, 100.0, 90.0, 90.0]; // and hits SL
        let c = vec![100.0, 100.0, 100.0, 100.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "stop_loss", "ambiguous bar must resolve pessimistically");
        approx_v(t.pnl, -5.0, 1e-9, "stopped at 95");
    }

    // ---------------------------------------------------------------- grid

    fn grid_settings(lower: f64, upper: f64, levels: usize, qty: f64) -> Settings {
        let mut s = settings_always_long();
        s.kind = "grid".into();
        s.sizing = Sizing::FixedQty { qty };
        s.grid = Some(GridConfig {
            lower,
            upper,
            levels,
            qty_per_level: qty,
            direction: "long".into(),
            anchor: "none".into(),
            ..GridConfig::default()
        });
        s
    }

    /// Grid geometry: `levels` lines evenly spaced in [lower, upper] ⇒ spacing =
    /// (upper-lower)/(levels-1). One down-and-up cycle across one cell = one round trip whose
    /// gross profit is exactly spacing × qty.
    #[test]
    fn v_grid_single_round_trip_profit() {
        // 5 levels in [90,110] ⇒ spacing 5: lines 90, 95, 100, 105, 110.
        let s = grid_settings(90.0, 110.0, 5, 1.0);
        let ts = ts_of(6);
        // Start at 100, dip to touch 95 (buy), rally back through 100 (sell one step up).
        let o = vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
        let h = vec![100.0, 100.0, 100.0, 101.0, 101.0, 101.0];
        let l = vec![100.0, 100.0, 94.0, 94.0, 99.0, 99.0];
        let c = vec![100.0, 100.0, 96.0, 100.0, 100.0, 100.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let done: Vec<_> = r.trades.iter().filter(|t| t.exit_reason == "take_profit").collect();
        assert!(!done.is_empty(), "expected at least one completed grid round trip");
        for t in &done {
            approx_v(t.pnl, 5.0, 1e-6, "one cell = one spacing of profit");
        }
    }

    /// Grid PnL reconciliation: final equity must equal capital + Σ trade pnl, including the
    /// leftover inventory closed at the end.
    #[test]
    fn v_grid_equity_reconciles() {
        let s = grid_settings(80.0, 120.0, 9, 1.0);
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.6).sin() * 15.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 2.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 2.0).collect();
        let c: Vec<f64> = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let sum: f64 = r.trades.iter().map(|t| t.pnl).sum();
        let g = r.grid.as_ref().expect("grid stats");
        // Grid deliberately does NOT force-close leftover inventory, so final_equity carries
        // an unrealized component. The reconciliation must therefore include it explicitly.
        approx_v(
            r.stats.final_equity,
            s.starting_capital + sum + g.end_inventory_value,
            1e-6,
            "grid equity = capital + Σpnl + unrealized inventory",
        );
        // And the gap must be exactly the reported inventory value, never anything else.
        approx_v(
            r.stats.final_equity - (s.starting_capital + sum),
            g.end_inventory_value,
            1e-6,
            "unexplained residual in grid equity",
        );
    }

    /// Grid stats must agree with the trade list: completed round trips = trades that closed
    /// for a grid reason (not the terminal liquidation).
    #[test]
    fn v_grid_stats_match_trades() {
        let s = grid_settings(80.0, 120.0, 9, 1.0);
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.6).sin() * 15.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 2.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 2.0).collect();
        let c: Vec<f64> = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let g = r.grid.as_ref().expect("grid stats present");
        let closed_rt = r.trades.iter().filter(|t| t.exit_reason == "take_profit").count();
        assert_eq!(g.round_trips as usize, closed_rt, "round_trips vs grid-closed trades");
    }

    /// Grid with a multiplier: each round trip's PnL must scale by the contract multiplier.
    #[test]
    fn v_grid_multiplier_scales_profit() {
        let mut s = grid_settings(90.0, 110.0, 5, 1.0);
        s.instrument = Instrument { multiplier: 10.0, lot_step: 0.0, min_qty: 0.0 };
        let ts = ts_of(6);
        let o = vec![100.0; 6];
        let h = vec![100.0, 100.0, 100.0, 101.0, 101.0, 101.0];
        let l = vec![100.0, 100.0, 94.0, 94.0, 99.0, 99.0];
        let c = vec![100.0, 100.0, 96.0, 100.0, 100.0, 100.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let done: Vec<_> = r.trades.iter().filter(|t| t.exit_reason == "take_profit").collect();
        assert!(!done.is_empty(), "expected a completed round trip");
        for t in &done {
            approx_v(t.pnl, 50.0, 1e-6, "spacing 5 x multiplier 10");
        }
    }

    // ---------------------------------------------------------------- stats

    /// profit_factor = gross wins / gross losses, computed from the trade list.
    #[test]
    fn v_profit_factor_matches_trades() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.05;
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.7).sin() * 9.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 4.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 4.0).collect();
        let c: Vec<f64> = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let wins: f64 = r.trades.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).sum();
        let losses: f64 = r.trades.iter().filter(|t| t.pnl < 0.0).map(|t| -t.pnl).sum();
        assert!(wins > 0.0 && losses > 0.0, "need both wins and losses");
        approx_v(r.stats.profit_factor, wins / losses, 1e-6, "profit factor");
        approx_v(r.stats.total_fees, r.trades.iter().map(|t| t.fees).sum::<f64>(), 1e-6, "total fees");
        approx_v(
            r.stats.avg_trade,
            r.trades.iter().map(|t| t.pnl).sum::<f64>() / r.trades.len() as f64,
            1e-6,
            "avg trade",
        );
    }

    /// win_rate must be wins / trades × 100, and wins + losses must account for every trade.
    #[test]
    fn v_win_rate_and_counts() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.05;
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.7).sin() * 9.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 4.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 4.0).collect();
        let c: Vec<f64> = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        assert_eq!(r.stats.trades as usize, r.trades.len(), "trade count");
        assert_eq!(
            (r.stats.wins + r.stats.losses) as usize,
            r.trades.len(),
            "wins+losses must partition the trades"
        );
        approx_v(
            r.stats.win_rate,
            r.stats.wins as f64 / r.stats.trades as f64 * 100.0,
            1e-6,
            "win rate",
        );
    }

    /// max_drawdown_pct must match a straightforward recomputation from the equity curve.
    #[test]
    fn v_max_drawdown_matches_curve() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 5.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.05;
        s.long.as_mut().unwrap().stop_loss_pct = 0.05;
        let n = 60;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.4).sin() * 12.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 4.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 4.0).collect();
        let c: Vec<f64> = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let mut peak = f64::MIN;
        let mut dd = 0.0f64;
        for p in &r.equity {
            peak = peak.max(p.equity);
            if peak > 0.0 {
                dd = dd.max((peak - p.equity) / peak * 100.0);
            }
        }
        approx_v(r.stats.max_drawdown_pct, dd, 1e-6, "max drawdown pct");
    }

    // ---------------------------------------------------------------- determinism

    /// The same settings and bars must produce byte-identical results across runs.
    #[test]
    fn v_determinism() {
        let mut s = settings_always_long();
        s.sizing = Sizing::PercentEquity { percent: 20.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.04;
        s.long.as_mut().unwrap().stop_loss_pct = 0.03;
        let n = 50;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.55).sin() * 10.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 3.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 3.0).collect();
        let c: Vec<f64> = o.clone();
        let b = mk("X", &ts, &o, &h, &l, &c);
        let r1 = run(&s, &b);
        let r2 = run(&s, &b);
        assert_eq!(
            r1.stats.final_equity.to_bits(),
            r2.stats.final_equity.to_bits(),
            "deterministic equity"
        );
        assert_eq!(r1.trades.len(), r2.trades.len(), "deterministic trade count");
    }

    /// A single-asset run and a one-asset portfolio run must be identical.
    #[test]
    fn v_single_vs_portfolio_of_one() {
        let mut s = settings_always_long();
        s.sizing = Sizing::PercentEquity { percent: 20.0 };
        s.long.as_mut().unwrap().take_profit_pct = 0.04;
        s.long.as_mut().unwrap().stop_loss_pct = 0.03;
        let n = 40;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.55).sin() * 10.0).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 3.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 3.0).collect();
        let c: Vec<f64> = o.clone();
        let b = mk("X", &ts, &o, &h, &l, &c);
        let r1 = run(&s, &b);
        let r2 = run_portfolio(&s, &[&b]);
        approx_v(r2.stats.final_equity, r1.stats.final_equity, 1e-9, "1-asset portfolio == single");
        assert_eq!(r1.trades.len(), r2.trades.len(), "same trades");
    }
}

/// Follow-up probes for the audit findings (2026-08-20).
#[cfg(test)]
mod verify2 {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }

    /// percent_equity with a contract multiplier: deploying 50% of a 10_000 account at price 100
    /// on a x50 contract is 5000 of notional ⇒ 5000 / (100 x 50) = 1 contract. Does the engine
    /// divide by the multiplier, or does it return 50 contracts (= 250_000 of notional)?
    #[test]
    fn v2_percent_equity_multiplier() {
        let closed: Vec<ClosedTrade> = vec![];
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 50.0, closed: &closed };
        let q = resolve_qty(&Sizing::PercentEquity { percent: 50.0 }, &c);
        approx_v(q, 1.0, 1e-9, "percent_equity must divide notional by (price x multiplier)");
    }

    /// Kelly sizing shares the same notional formula, so it should share the same treatment.
    #[test]
    fn v2_kelly_multiplier() {
        let closed: Vec<ClosedTrade> = (0..30)
            .map(|i| ClosedTrade { ret: if i % 2 == 0 { 0.10 } else { -0.05 }, win: i % 2 == 0 })
            .collect();
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 50.0, closed: &closed };
        let q = resolve_qty(
            &Sizing::Kelly { fraction: 0.5, window: 30, cap_pct: 20.0, warmup: None },
            &c,
        );
        // p=0.5, b=0.10/0.05=2 ⇒ kelly = 0.5 - 0.5/2 = 0.25; half-kelly = 0.125 → capped at 0.20,
        // so frac = 0.125 ⇒ notional = 1250 ⇒ on a x50 contract that is 0.25 contracts.
        approx_v(q, 0.25, 1e-9, "kelly must divide notional by (price x multiplier)");
    }

    /// equity_tiers with metric percent_equity: same formula, same question.
    #[test]
    fn v2_tiers_percent_equity_multiplier() {
        let closed: Vec<ClosedTrade> = vec![];
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 50.0, closed: &closed };
        let q = resolve_qty(
            &Sizing::EquityTiers {
                metric: "percent_equity".into(),
                tiers: vec![Tier { above: 0.0, value: 50.0 }],
            },
            &c,
        );
        approx_v(q, 1.0, 1e-9, "tiers/percent_equity must divide by (price x multiplier)");
    }

    /// equity_tiers with metric risk_pct routes through the same qty_for_risk as Sizing::Risk.
    #[test]
    fn v2_tiers_risk_multiplier() {
        let closed: Vec<ClosedTrade> = vec![];
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: Some(98.0), leverage: 1.0, mult: 50.0, closed: &closed };
        let q = resolve_qty(
            &Sizing::EquityTiers { metric: "risk_pct".into(), tiers: vec![Tier { above: 0.0, value: 1.0 }] },
            &c,
        );
        approx_v(q, 1.0, 1e-9, "tiers/risk_pct must divide by (|entry-stop| x multiplier)");
    }

    /// PercentEquity without a multiplier is the baseline and must be unaffected by any fix.
    #[test]
    fn v2_percent_equity_baseline() {
        let closed: Vec<ClosedTrade> = vec![];
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 1.0, closed: &closed };
        approx_v(
            resolve_qty(&Sizing::PercentEquity { percent: 50.0 }, &c),
            50.0,
            1e-9,
            "multiplier-1 baseline unchanged",
        );
    }

    /// A short gapping *up* through its stop: same liquidity question as the long case.
    #[test]
    fn v2_short_gap_through_stop() {
        let mut s = settings_always_short();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.short.as_mut().unwrap().stop_loss_pct = 0.05; // short from 100 ⇒ stop = 105
        let ts: Vec<String> = (0..4).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect();
        let o = vec![100.0, 100.0, 130.0, 130.0]; // gaps far above the 105 stop
        let h = vec![100.0, 100.0, 132.0, 132.0];
        let l = vec![100.0, 100.0, 128.0, 128.0];
        let c = vec![100.0, 100.0, 130.0, 130.0];
        let r = run(&s, &tests_bars(&ts, &o, &h, &l, &c));
        let t = r.trades.iter().find(|t| t.exit_reason == "stop_loss").expect("stopped out");
        assert!(
            t.exit_price >= 130.0 - 1e-9,
            "short gap fill must not be better than the open: got {}",
            t.exit_price
        );
    }
}

/// Round 2 of the audit (2026-08-20): stats classification, OOS split, funding, portfolio
/// limits, pyramiding gates, ATR stops, exit priority, optimizer decode.
#[cfg(test)]
mod verify3 {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }

    fn ts_of(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect()
    }

    fn trade_with(pnl: f64, ret: f64) -> Trade {
        Trade {
            ticker: "X".into(),
            entry_ts: "2024-01-01T00:00:00Z".into(),
            exit_ts: "2024-01-02T00:00:00Z".into(),
            entry_price: 100.0,
            exit_price: 100.0,
            qty: 1.0,
            entries: 1,
            direction: "long".into(),
            exit_reason: "end".into(),
            pnl,
            fees: 0.0,
            return_pct: ret,
            bars_held: 1,
            mae: 0.0,
            mfe: 0.0,
        }
    }

    /// A breakeven trade (pnl exactly 0) is neither a win nor a loss. Counting it as a win
    /// inflates win_rate and gross_profit.
    #[test]
    fn v3_breakeven_trade_is_not_a_win() {
        let trs = vec![trade_with(0.0, 0.0), trade_with(-10.0, -1.0)];
        let st = side_stats(trs.iter());
        assert_eq!(st.wins, 0, "a 0-pnl trade must not count as a win");
        approx_v(st.win_rate, 0.0, 1e-9, "win_rate with one breakeven and one loss");
    }

    /// profit_factor must not be inflated by breakeven trades landing in gross_profit.
    #[test]
    fn v3_breakeven_not_in_gross_profit() {
        let trs = vec![trade_with(0.0, 0.0), trade_with(10.0, 1.0), trade_with(-5.0, -0.5)];
        let st = side_stats(trs.iter());
        approx_v(st.gross_profit, 10.0, 1e-9, "gross_profit excludes the breakeven");
        approx_v(st.profit_factor, 2.0, 1e-9, "10 / 5");
    }

    /// A breakeven trade interrupts a losing streak rather than extending it, which is what
    /// `analysis.rs` and `analysis/trades.js` already do — the engine must agree with them.
    /// The bug being guarded against is the OLD behavior, where a breakeven counted as a WIN
    /// and so also reset the loss streak while inflating the win streak.
    #[test]
    fn v3_breakeven_interrupts_streaks_without_counting_as_a_win() {
        let trs = vec![trade_with(-1.0, 0.0), trade_with(0.0, 0.0), trade_with(-1.0, 0.0)];
        let st = side_stats(trs.iter());
        assert_eq!(st.max_consec_losses, 1, "breakeven interrupts the losing streak");
        assert_eq!(st.max_consec_wins, 0, "a breakeven is never a win");
        assert_eq!(st.wins, 0);
        assert_eq!(st.losses, 2);
        assert_eq!(st.breakeven, 1);
        assert_eq!(st.wins + st.losses + st.breakeven, st.trades, "three-way partition");
    }

    /// profit_factor with zero losses: dividing by zero must not silently report 0 (which reads
    /// as "worst possible") for a strategy that never lost.
    #[test]
    fn v3_profit_factor_all_wins() {
        let trs = vec![trade_with(10.0, 1.0), trade_with(5.0, 0.5)];
        let st = side_stats(trs.iter());
        assert_eq!(
            st.profit_factor, PROFIT_FACTOR_NO_LOSSES,
            "all-wins profit factor must be the finite no-losses sentinel"
        );
        // It must survive the wire as a NUMBER: infinity would serialize to null and the UI
        // would hide the field, and the optimizer would sink it to the bottom of the ranking.
        let j = serde_json::to_string(&st).unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(
            v.get("profit_factor").and_then(|x| x.as_f64()).is_some_and(|x| x > 1e6),
            "profit_factor must cross the wire as a large number, got {:?}",
            v.get("profit_factor")
        );
        // And it must survive the optimizer's f32 row storage without becoming infinite.
        assert!(
            (PROFIT_FACTOR_NO_LOSSES as f32).is_finite(),
            "sentinel must stay finite as f32"
        );
    }

    /// OOS split: a trade must be attributed to the segment consistently with the equity slice
    /// that contains its PnL. A trade entering in-sample and exiting out-of-sample puts its whole
    /// PnL in the in-sample block while the equity move lands in the OOS slice.
    #[test]
    fn v3_oos_straddling_trade_consistency() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.oos_split_pct = 0.5;
        s.pyramiding = 1;
        // A single trade held across the whole run: enters bar 1, exits at the last bar.
        let n = 10;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 5.0).collect();
        let h = o.clone();
        let l = o.clone();
        let c = o.clone();
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let oos = r.oos.as_ref().expect("oos present");
        // The two segments' net_pnl must add up to the whole run's net PnL, wherever the
        // straddling trade is attributed. If they do not, the split double-counts or loses PnL.
        approx_v(
            oos.in_sample.net_pnl + oos.out_sample.net_pnl,
            r.trades.iter().map(|t| t.pnl).sum::<f64>(),
            1e-6,
            "IS + OOS net_pnl must equal total",
        );
        // Every trade must land in exactly one segment (no double counting, none dropped).
        assert_eq!(
            oos.in_sample.trades + oos.out_sample.trades,
            r.trades.len(),
            "the split must partition the trade list"
        );
        // A trade is attributed to the segment it CLOSED in, so a straddling trade is
        // out-of-sample. The equity-curve slices still carry the position's mark-to-market
        // drift bar by bar, so `final_equity` deltas need not equal `net_pnl` while a position
        // is open across the boundary — that residual is the unrealized part, by construction.
        for t in &r.trades {
            let in_oos = t.exit_ts >= *oos.split_ts.as_ref().unwrap();
            assert!(
                !in_oos || oos.out_sample.trades > 0,
                "a trade closing at/after the split must be counted out-of-sample"
            );
        }
    }

    fn mk<'a>(tk: &'a str, ts: &'a [String], o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> Bars<'a> {
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c }
    }

    /// Funding: a long held for a known span at a known annual rate must be charged the
    /// arithmetic amount. 10 bars of 1 day, notional 100, 36.525%/yr ⇒ 0.1%/day ⇒ 0.1/day.
    #[test]
    fn v3_funding_arithmetic() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.funding = Funding { annual_rate_pct: 36.525, interval_hours: 24.0 };
        let n = 6;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        // Position opens at bar 1 and is held to bar 5 ⇒ 4 daily accruals of 100 x 0.001 = 0.1.
        approx_v(r.total_funding, -0.4, 1e-6, "4 days of funding on 100 notional");
    }

    /// Funding must be reflected in final equity, not just reported.
    #[test]
    fn v3_funding_hits_equity() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        let n = 6;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let no_f = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        s.funding = Funding { annual_rate_pct: 36.525, interval_hours: 24.0 };
        let with_f = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        approx_v(
            with_f.stats.final_equity - no_f.stats.final_equity,
            with_f.total_funding,
            1e-6,
            "equity delta must equal funding",
        );
    }

    /// max_exposure_pct must actually cap total open notional as a share of equity.
    #[test]
    fn v3_max_exposure_caps_notional() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 10.0 }; // 10 x 100 = 1000 notional per asset
        s.starting_capital = 10_000.0;
        s.risk = Risk { max_exposure_pct: Some(15.0), ..Risk::default() }; // 1500 max
        let n = 8;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let a = mk("A", &ts, &px, &px, &px, &px);
        let b = mk("B", &ts, &px, &px, &px, &px);
        let c = mk("C", &ts, &px, &px, &px, &px);
        let r = run_portfolio(&s, &[&a, &b, &c]);
        // Peak concurrent open notional, reconstructed from the trade list: a trade is open on
        // every bar in [entry_ts, exit_ts). Flat price ⇒ notional is 1000 per open position.
        let mut peak_open = 0usize;
        for t0 in &ts {
            let open = r
                .trades
                .iter()
                .filter(|t| t.entry_ts <= *t0 && t.exit_ts > *t0)
                .count();
            peak_open = peak_open.max(open);
        }
        let peak_notional = peak_open as f64 * 1000.0;
        // The cap is 15% of 10_000 = 1500. Opening a 1000-notional lot when 1000 is already
        // open takes the book to 2000, which is over the cap and must be refused.
        assert!(
            peak_notional <= 1500.0,
            "exposure cap 1500 breached: {peak_open} concurrent positions = {peak_notional} notional"
        );
    }

    /// max_open_positions is a hard ceiling on simultaneous positions.
    #[test]
    fn v3_max_open_positions_ceiling() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.risk = Risk { max_open_positions: Some(2), ..Risk::default() };
        let n = 8;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let a = mk("A", &ts, &px, &px, &px, &px);
        let b = mk("B", &ts, &px, &px, &px, &px);
        let c = mk("C", &ts, &px, &px, &px, &px);
        let r = run_portfolio(&s, &[&a, &b, &c]);
        let tickers: std::collections::BTreeSet<&str> =
            r.trades.iter().map(|t| t.ticker.as_str()).collect();
        assert!(tickers.len() <= 2, "at most 2 assets may hold a position, got {tickers:?}");
    }

    /// ATR stop distance: ATR sampled at the entry bar times the multiple, fixed for the trade.
    #[test]
    fn v3_atr_stop_distance_is_fixed() {
        let rule = Stop { kind: "atr".into(), value: 2.0, period: 14 };
        approx_v(
            stop_distance(&rule, 100.0, Some(3.0)).unwrap(),
            6.0,
            1e-9,
            "ATR 3 x multiple 2",
        );
        // Unlike pct, the ATR distance must not scale with the average entry.
        approx_v(
            stop_distance(&rule, 500.0, Some(3.0)).unwrap(),
            6.0,
            1e-9,
            "ATR distance independent of avg",
        );
    }

    /// pct stop distance re-anchors on the (pyramiding-updated) average entry.
    #[test]
    fn v3_pct_stop_distance_scales_with_avg() {
        let rule = Stop { kind: "pct".into(), value: 0.05, period: 0 };
        approx_v(stop_distance(&rule, 100.0, None).unwrap(), 5.0, 1e-9, "5% of 100");
        approx_v(stop_distance(&rule, 200.0, None).unwrap(), 10.0, 1e-9, "5% of 200");
    }

    /// Pyramiding min-distance gate: an add is refused until price has moved favorably by the
    /// configured fraction against the current average.
    #[test]
    fn v3_pyramid_min_distance_gate() {
        let mut s = settings_always_long();
        s.pyramiding = 3;
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.pyramid_steps = PyramidSteps { min_distance_pct: 0.10, ..PyramidSteps::default() };
        // Flat price: no favorable move ever, so no add may ever fire.
        let n = 8;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        assert_eq!(r.trades[0].entries, 1, "min-distance gate must block adds on a flat tape");
    }

    /// Optimizer mixed-radix decode: every trial index maps to a distinct combination, and the
    /// last axis varies fastest.
    #[test]
    fn v3_optimizer_radix_decode() {
        use super::optimize::{digits, Axis};
        let ax = |vals: Vec<f64>| Axis {
            path: "x".into(),
            paths: vec![],
            label: String::new(),
            scale: 1.0,
            unit: String::new(),
            values: vals.into_iter().map(|v| serde_json::json!(v)).collect(),
        };
        let axes = vec![ax(vec![1.0, 2.0, 3.0]), ax(vec![10.0, 20.0])];
        // 3 x 2 = 6 combinations, all distinct.
        let all: std::collections::BTreeSet<Vec<u16>> = (0..6).map(|i| digits(i, &axes)).collect();
        assert_eq!(all.len(), 6, "all 6 trials must be distinct combinations");
        // Last axis fastest: trial 0 = [0,0], trial 1 = [0,1], trial 2 = [1,0].
        assert_eq!(digits(0, &axes), vec![0, 0]);
        assert_eq!(digits(1, &axes), vec![0, 1]);
        assert_eq!(digits(2, &axes), vec![1, 0]);
    }

    /// Sharpe annualization: a constant per-bar return has zero variance ⇒ None, not a bogus
    /// infinity. And a known series must annualize by sqrt(bars per year).
    #[test]
    fn v3_sharpe_constant_returns_is_none() {
        let eq: Vec<EquityPoint> = (0..10)
            .map(|i| EquityPoint {
                ts: format!("2024-01-{:02}T00:00:00Z", i + 1),
                equity: 10_000.0 * 1.01_f64.powi(i),
            })
            .collect();
        let (sharpe, _) = risk_ratios(&eq);
        assert!(sharpe.is_none(), "zero-variance returns must yield None, got {sharpe:?}");
    }

    /// Sortino downside deviation must use only negative returns as the deviation basis.
    #[test]
    fn v3_sortino_all_positive_is_none() {
        let eq: Vec<EquityPoint> = (0..10)
            .map(|i| EquityPoint {
                ts: format!("2024-01-{:02}T00:00:00Z", i + 1),
                equity: 10_000.0 + i as f64 * 100.0,
            })
            .collect();
        let (_, sortino) = risk_ratios(&eq);
        assert!(sortino.is_none(), "no downside ⇒ None, got {sortino:?}");
    }
}

/// Round 3: confirmations of the round-2 findings.
#[cfg(test)]
mod verify4 {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }

    fn ts_of(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect()
    }
    fn mk<'a>(tk: &'a str, ts: &'a [String], o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> Bars<'a> {
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c }
    }

    /// The per-asset exposure cap has the same shape as the portfolio one: does it count the
    /// lot being added? Cap 15% of 10_000 = 1500; each add is 1000 of notional.
    #[test]
    fn v4_max_exposure_per_asset_counts_new_lot() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 10.0 };
        s.pyramiding = 5;
        s.starting_capital = 10_000.0;
        s.risk = Risk { max_exposure_per_asset_pct: Some(15.0), ..Risk::default() };
        let n = 10;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        let t = &r.trades[0];
        let notional = t.qty * 100.0;
        assert!(
            notional <= 1500.0,
            "per-asset cap 1500 breached: {} units = {notional} notional ({} adds)",
            t.qty,
            t.entries
        );
    }

    /// Sharpe must not explode on returns that are constant up to float noise. A perfectly
    /// smooth compounding curve has mathematically zero variance.
    #[test]
    fn v4_sharpe_noise_variance_guard() {
        let eq: Vec<EquityPoint> = (0..30)
            .map(|i| EquityPoint {
                ts: format!("2024-{:02}-{:02}T00:00:00Z", i / 28 + 1, i % 28 + 1),
                equity: 10_000.0 * 1.005_f64.powi(i),
            })
            .collect();
        let (sharpe, _) = risk_ratios(&eq);
        match sharpe {
            None => {}
            Some(v) => assert!(
                v.abs() < 1e4,
                "float-noise variance produced an absurd Sharpe: {v}"
            ),
        }
    }

    /// OOS: the equity curve is partitioned by timestamp while trades are partitioned by
    /// entry_ts, so a straddling trade's PnL and its equity move land in different segments.
    /// Pin the discrepancy explicitly.
    #[test]
    fn v4_oos_equity_vs_trade_attribution() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.oos_split_pct = 0.5;
        s.pyramiding = 1;
        let n = 10;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 5.0).collect();
        let r = run(&s, &mk("X", &ts, &o, &o, &o, &o));
        let oos = r.oos.as_ref().expect("oos");
        // One trade, entered in-sample, exited out-of-sample.
        assert_eq!(r.trades.len(), 1, "single straddling trade");
        // It closed after the split, so it belongs to the out-of-sample block. Before the fix
        // it was counted in-sample (by entry time) while the equity move it caused sat in the
        // OOS slice, so the OOS block reported zero PnL over a segment where equity moved.
        assert_eq!(oos.in_sample.trades, 0, "straddling trade must NOT be in-sample");
        assert_eq!(oos.out_sample.trades, 1, "straddling trade is attributed to its exit");
        assert!(
            oos.out_sample.net_pnl.abs() > 1e-9,
            "the OOS block must now carry the trade's PnL, got {}",
            oos.out_sample.net_pnl
        );
        approx_v(
            oos.in_sample.net_pnl + oos.out_sample.net_pnl,
            r.trades.iter().map(|t| t.pnl).sum::<f64>(),
            1e-6,
            "the two blocks must still sum to the total",
        );
    }
}

/// Round 4: mechanics not yet exercised — reversal accounting, breakeven stop override,
/// MAE/MFE sign conventions, Kelly window, fee-per-trade on pyramided positions.
#[cfg(test)]
mod verify5 {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }
    fn ts_of(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect()
    }
    fn mk<'a>(tk: &'a str, ts: &'a [String], o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> Bars<'a> {
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c }
    }

    /// `per=trade` fixed fee on a pyramided position: the docs say the fee is per *trade*, but
    /// each lot calls fee_for independently. Three adds at a flat 2.0 fee ⇒ is it 2 or 6?
    #[test]
    fn v5_per_trade_fee_on_pyramided_entries() {
        let mut s = settings_always_long();
        s.pyramiding = 3;
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.fees = Fees { amount_kind: "fixed".into(), per: "trade".into(), amount: 2.0 };
        let n = 8;
        let ts = ts_of(n);
        let px = vec![100.0; n];
        let r = run(&s, &mk("X", &ts, &px, &px, &px, &px));
        let t = &r.trades[0];
        assert_eq!(t.entries, 3, "three lots");
        // 3 entry fills + 1 exit fill, each a separate order ⇒ 4 x 2.0 = 8.0.
        approx_v(t.fees, 8.0, 1e-9, "per-trade fee charged once per fill");
    }

    /// MAE/MFE sign convention: engine stores both as positive magnitudes.
    #[test]
    fn v5_mae_mfe_are_positive_magnitudes() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.pyramiding = 1;
        let ts = ts_of(6);
        let o = vec![100.0; 6];
        let h = vec![100.0, 100.0, 120.0, 120.0, 120.0, 120.0];
        let l = vec![100.0, 100.0, 80.0, 80.0, 80.0, 80.0];
        let c = vec![100.0; 6];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert!(t.mae >= 0.0, "mae must be a positive magnitude, got {}", t.mae);
        assert!(t.mfe >= 0.0, "mfe must be a positive magnitude, got {}", t.mfe);
        // Entry 100, low 80 ⇒ worst open loss = 20 per unit.
        approx_v(t.mae, 20.0, 1e-6, "mae = entry - lowest low");
        approx_v(t.mfe, 20.0, 1e-6, "mfe = highest high - entry");
    }

    /// MAE/MFE must scale with the contract multiplier, since they are stated in account
    /// currency (the doc comment on Pos says "in account currency").
    #[test]
    fn v5_mae_mfe_scale_with_multiplier() {
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.pyramiding = 1;
        s.instrument = Instrument { multiplier: 10.0, lot_step: 0.0, min_qty: 0.0 };
        let ts = ts_of(6);
        let o = vec![100.0; 6];
        let h = vec![100.0, 100.0, 120.0, 120.0, 120.0, 120.0];
        let l = vec![100.0, 100.0, 80.0, 80.0, 80.0, 80.0];
        let c = vec![100.0; 6];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        approx_v(t.mae, 200.0, 1e-6, "mae in account currency = 20 x mult 10");
    }

    /// Kelly: with a full window of trades the sizing must use the rolling window, and the
    /// documented cap must bind. p=1 (all wins) ⇒ b infinite ⇒ kelly = 1 ⇒ capped at cap_pct.
    #[test]
    fn v5_kelly_cap_binds() {
        let closed: Vec<ClosedTrade> = (0..30).map(|_| ClosedTrade { ret: 0.1, win: true }).collect();
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 1.0, closed: &closed };
        let q = resolve_qty(
            &Sizing::Kelly { fraction: 0.5, window: 30, cap_pct: 20.0, warmup: None },
            &c,
        );
        // cap 20% of 10_000 = 2000 notional at price 100 ⇒ 20 units.
        approx_v(q, 20.0, 1e-9, "kelly capped at cap_pct");
    }

    /// Negative Kelly (losing edge) must skip the entry, not size it negative or tiny.
    #[test]
    fn v5_kelly_negative_skips() {
        // p = 0.2 wins of 0.01, losses of 0.10 ⇒ b = 0.1, kelly = 0.2 - 0.8/0.1 < 0.
        let closed: Vec<ClosedTrade> = (0..30)
            .map(|i| if i % 5 == 0 { ClosedTrade { ret: 0.01, win: true } } else { ClosedTrade { ret: -0.10, win: false } })
            .collect();
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 1.0, closed: &closed };
        let q = resolve_qty(
            &Sizing::Kelly { fraction: 0.5, window: 30, cap_pct: 20.0, warmup: None },
            &c,
        );
        approx_v(q, 0.0, 1e-12, "negative kelly must skip");
    }

    /// Stop-and-reverse: closing long and opening short on the same bar must settle the long's
    /// PnL and leave exactly one open position, with total equity conserved.
    #[test]
    fn v5_stop_and_reverse_conserves_equity() {
        let mut s = settings_always_long();
        s.mode = Mode::Both;
        s.stop_and_reverse = true;
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        // Long entry always true; short entry always true too ⇒ immediate reversal each bar.
        s.short = Some(Side {
            signal: None,
            entry: Some(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] }),
            exit: None,
            stop_loss_pct: 0.0,
            take_profit_pct: 0.0,
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
            exit_on_reverse: false,
        });
        let n = 10;
        let ts = ts_of(n);
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let r = run(&s, &mk("X", &ts, &o, &o, &o, &o));
        let sum: f64 = r.trades.iter().map(|t| t.pnl).sum();
        approx_v(
            r.stats.final_equity,
            s.starting_capital + sum,
            1e-6,
            "reversal chain must conserve equity",
        );
    }

    /// after_add_sl = breakeven: once an add fills, the stop moves to the new average entry.
    #[test]
    fn v5_after_add_breakeven_stop() {
        let mut s = settings_always_long();
        s.pyramiding = 2;
        s.sizing = Sizing::FixedQty { qty: 1.0 };
        s.pyramid_steps = PyramidSteps { after_add_sl: "breakeven".into(), ..PyramidSteps::default() };
        s.long.as_mut().unwrap().stop_loss_pct = 0.50; // a far stop, so only the override can fire
        // bar1 fill 100, bar2 fill 110 ⇒ avg 105 ⇒ stop moves to 105. bar3 dips to 104.
        let ts = ts_of(6);
        let o = vec![100.0, 100.0, 110.0, 110.0, 110.0, 110.0];
        let h = vec![100.0, 100.0, 110.0, 110.0, 110.0, 110.0];
        let l = vec![100.0, 100.0, 110.0, 104.0, 104.0, 104.0];
        let c = vec![100.0, 100.0, 110.0, 106.0, 106.0, 106.0];
        let r = run(&s, &mk("X", &ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.entries, 2, "two lots");
        assert_eq!(t.exit_reason, "stop_loss", "breakeven override must fire");
        approx_v(t.exit_price, 105.0, 1e-6, "stop at the new average");
        approx_v(t.pnl, 0.0, 1e-6, "breakeven exit ⇒ zero pnl");
    }

    /// Instrument lot rounding must round DOWN (never up past the risk budget).
    #[test]
    fn v5_lot_rounding_rounds_down() {
        let inst = Instrument { multiplier: 1.0, lot_step: 0.5, min_qty: 0.0 };
        approx_v(inst.round_qty(1.7).unwrap(), 1.5, 1e-12, "1.7 ⇒ 1.5");
        approx_v(inst.round_qty(2.0).unwrap(), 2.0, 1e-12, "exact multiple kept");
        assert!(inst.round_qty(0.3).is_none(), "below one step ⇒ refused");
    }

    /// min_qty must refuse anything under it, even after rounding.
    #[test]
    fn v5_min_qty_refuses() {
        let inst = Instrument { multiplier: 1.0, lot_step: 0.1, min_qty: 1.0 };
        assert!(inst.round_qty(0.9).is_none(), "0.9 < min 1.0 ⇒ refused");
        assert!(inst.round_qty(1.05).is_some(), "1.0 after rounding ⇒ allowed");
    }
}

/// Cross-check harness (audit 2026-08-20): dumps engine results for a parameter sweep as JSON
/// so an independent reference implementation can diff them trade by trade.
/// Run with: `cargo test -p otw-core backtest::xcheck::dump -- --ignored --nocapture`
#[cfg(test)]
mod xcheck {
    use super::tests::*;
    use super::*;

    /// Deterministic pseudo-random price path (LCG), so Rust and Python see identical bars.
    fn path(n: usize, seed: u64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let mut st = seed;
        let mut next = || {
            st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            // >> 32 keeps 32 bits: /2^31 lands in [0, 2), minus 1 is a symmetric [-1, 1).
            // Taking 31 bits (>> 33) made this [-1, 0) and every path a straight line down.
            ((st >> 32) as f64) / ((1u64 << 31) as f64) - 1.0
        };
        let mut o = Vec::with_capacity(n);
        let mut hh = Vec::with_capacity(n);
        let mut ll = Vec::with_capacity(n);
        let mut cc = Vec::with_capacity(n);
        let mut px = 100.0f64;
        for _ in 0..n {
            let drift = next() * 2.0;
            let open = px;
            let close = (px + drift).max(1.0);
            let hi = open.max(close) + next().abs() * 1.5;
            let lo = (open.min(close) - next().abs() * 1.5).max(0.5);
            o.push(open);
            hh.push(hi);
            ll.push(lo);
            cc.push(close);
            px = close;
        }
        (o, hh, ll, cc)
    }

    fn trail_json(t: &Trail) -> serde_json::Value {
        serde_json::json!({
            "kind": t.kind,
            "value": t.value,
            "activate_pct": t.activate_pct,
            "breakeven_pct": t.breakeven_pct,
        })
    }

    #[test]
    #[ignore]
    fn dump() {
        let n = 120;
        let (o, h, l, c) = path(n, 42);
        let ts: Vec<String> = (0..n)
            .map(|i| format!("2024-{:02}-{:02}T00:00:00Z", i / 28 + 1, i % 28 + 1))
            .collect();
        let b = Bars { ticker: "X", ts: &ts, open: &o, high: &h, low: &l, close: &c, volume: &c };

        // Emit the bars once so the reference sees identical input.
        let bars_json = serde_json::json!({
            "ts": ts, "open": o, "high": h, "low": l, "close": c,
        });
        println!("###BARS###");
        println!("{}", serde_json::to_string(&bars_json).unwrap());

        // Parameter sweep: each case is (name, settings-mutator, entry-signal description).
        // Entry signal is "always true" so the reference can reproduce it exactly without
        // reimplementing indicators; the variation under test is the money/fill machinery.
        let mut cases: Vec<(String, Settings)> = Vec::new();

        let mk = || settings_always_long();

        for &qty in &[1.0f64, 3.5] {
            for &spread in &[0.0f64, 0.002] {
                for &(fk, fp, fa) in &[
                    ("fixed", "trade", 0.0),
                    ("fixed", "trade", 1.5),
                    ("pct", "trade", 0.05),
                    ("fixed", "unit", 0.25),
                    ("pct", "unit", 0.02),
                ] {
                    for &sl in &[0.0f64, 0.02, 0.05] {
                        for &tp in &[0.0f64, 0.03, 0.10] {
                            for &pyr in &[1usize, 3] {
                                let mut s = mk();
                                s.sizing = Sizing::FixedQty { qty };
                                s.spread_pct = spread;
                                s.fees = Fees {
                                    amount_kind: fk.into(),
                                    per: fp.into(),
                                    amount: fa,
                                };
                                s.pyramiding = pyr;
                                {
                                    let sd = s.long.as_mut().unwrap();
                                    sd.stop_loss_pct = sl;
                                    sd.take_profit_pct = tp;
                                }
                                let name = format!(
                                    "qty{qty}_sp{spread}_{fk}-{fp}-{fa}_sl{sl}_tp{tp}_pyr{pyr}"
                                );
                                cases.push((name, s));
                            }
                        }
                    }
                }
            }
        }

        // A few sizing / leverage / instrument variants on top.
        for &pct in &[5.0f64, 25.0] {
            for &lev in &[1.0f64, 3.0] {
                let mut s = mk();
                s.sizing = Sizing::PercentEquity { percent: pct };
                s.leverage = lev;
                s.long.as_mut().unwrap().stop_loss_pct = 0.03;
                s.long.as_mut().unwrap().take_profit_pct = 0.06;
                cases.push((format!("pctEq{pct}_lev{lev}"), s));
            }
        }
        for &step in &[0.1f64, 1.0] {
            let mut s = mk();
            s.sizing = Sizing::PercentEquity { percent: 10.0 };
            s.instrument = Instrument { multiplier: 1.0, lot_step: step, min_qty: 0.0 };
            s.long.as_mut().unwrap().stop_loss_pct = 0.04;
            cases.push((format!("lotstep{step}"), s));
        }
        // Multiplier variants: these hit the sizing paths BUG-1 is about, so the reference
        // (which divides by price x multiplier) is expected to disagree wherever sizing is
        // notional- or risk-based. FixedQty is multiplier-independent and must still agree.
        for &m in &[10.0f64, 50.0] {
            let mut s = mk();
            s.sizing = Sizing::FixedQty { qty: 1.0 };
            s.instrument = Instrument { multiplier: m, lot_step: 0.0, min_qty: 0.0 };
            s.long.as_mut().unwrap().stop_loss_pct = 0.03;
            s.long.as_mut().unwrap().take_profit_pct = 0.06;
            cases.push((format!("mult{m}_fixedqty"), s));
        }
        // Gap-heavy path: BUG-2 territory. The reference fills a gapped stop at the stop price
        // too (it mirrors the documented engine behavior), so agreement here confirms the
        // reference matches the ENGINE, and the separate unit test is what proves the
        // behavior is wrong. Kept so the harness covers the branch.
        for &sl in &[0.01f64, 0.03] {
            let mut s = mk();
            s.sizing = Sizing::FixedQty { qty: 2.0 };
            s.long.as_mut().unwrap().stop_loss_pct = sl;
            cases.push((format!("gapstop{sl}"), s));
        }
        // Trailing stop, long and short: distance kind × the two threshold steps × a fixed stop
        // beside it × costs × pyramiding. `pct` and `abs` are the whole vocabulary.
        for short in [false, true] {
            for &(tk, tv) in &[("pct", 0.02f64), ("pct", 0.06), ("abs", 1.5), ("abs", 4.0)] {
                for &act in &[0.0f64, 0.03] {
                    for &be in &[0.0f64, 0.02] {
                        for &sl in &[0.0f64, 0.05] {
                            for &(spread, fa, pyr) in
                                &[(0.0f64, 0.0f64, 1usize), (0.002, 1.5, 1), (0.0, 0.05, 3)]
                            {
                                let mut s =
                                    if short { settings_always_short() } else { settings_always_long() };
                                s.sizing = Sizing::FixedQty { qty: 2.0 };
                                s.spread_pct = spread;
                                s.pyramiding = pyr;
                                s.fees = Fees {
                                    amount_kind: if fa == 0.05 { "pct".into() } else { "fixed".into() },
                                    per: "trade".into(),
                                    amount: fa,
                                };
                                {
                                    let sd =
                                        if short { s.short.as_mut() } else { s.long.as_mut() }.unwrap();
                                    sd.stop_loss_pct = sl;
                                    sd.trailing_stop = Some(Trail {
                                        kind: tk.into(),
                                        value: tv,
                                        activate_pct: act,
                                        breakeven_pct: be,
                                    });
                                }
                                let side = if short { "S" } else { "L" };
                                cases.push((
                                    format!(
                                        "trail{side}_{tk}{tv}_act{act}_be{be}_sl{sl}_sp{spread}_f{fa}_pyr{pyr}"
                                    ),
                                    s,
                                ));
                            }
                        }
                    }
                }
            }
        }

        // exit_on_reverse with a never-true entry after the first bar is not expressible with
        // an always-true signal, so exercise the flag itself (it must never fire here).
        {
            let mut s = mk();
            s.sizing = Sizing::FixedQty { qty: 1.0 };
            s.long.as_mut().unwrap().exit_on_reverse = true;
            s.long.as_mut().unwrap().stop_loss_pct = 0.04;
            cases.push(("exit_on_reverse_alwaystrue".to_string(), s));
        }
        // Risk sizing WITH a multiplier: the reference sizes correctly (per-unit risk includes
        // the multiplier), so any disagreement here is BUG-1 measured end to end in currency.
        for &m in &[1.0f64, 10.0] {
            for &rp in &[0.5f64, 2.0] {
                let mut s = mk();
                s.sizing = Sizing::Risk { risk_pct: rp };
                s.instrument = Instrument { multiplier: m, lot_step: 0.0, min_qty: 0.0 };
                s.long.as_mut().unwrap().stop_loss_pct = 0.03;
                s.long.as_mut().unwrap().take_profit_pct = 0.06;
                cases.push((format!("risk{rp}_mult{m}"), s));
            }
        }
        // Percent-equity WITH a multiplier: same, for the notional branch of BUG-1.
        for &m in &[1.0f64, 10.0] {
            let mut s = mk();
            s.sizing = Sizing::PercentEquity { percent: 10.0 };
            s.instrument = Instrument { multiplier: m, lot_step: 0.0, min_qty: 0.0 };
            s.long.as_mut().unwrap().stop_loss_pct = 0.03;
            cases.push((format!("pctEq_mult{m}"), s));
        }
        for &slip in &[0.001f64, 0.005] {
            let mut s = mk();
            s.sizing = Sizing::FixedQty { qty: 2.0 };
            s.slippage = Slippage { kind: "pct".into(), value: slip, tick_size: 0.0 };
            s.long.as_mut().unwrap().stop_loss_pct = 0.03;
            s.long.as_mut().unwrap().take_profit_pct = 0.05;
            cases.push((format!("slip{slip}"), s));
        }

        // ── Short-side sweep: same machinery mirrored. Emitted separately so the reference
        // can run its short model against it.
        let mut short_cases: Vec<(String, Settings)> = Vec::new();
        for &qty in &[1.0f64, 2.5] {
            for &spread in &[0.0f64, 0.002] {
                for &(fk, fp, fa) in &[("fixed", "trade", 0.0), ("pct", "trade", 0.05), ("fixed", "unit", 0.25)] {
                    for &sl in &[0.0f64, 0.03] {
                        for &tp in &[0.0f64, 0.05] {
                            for &pyr in &[1usize, 2] {
                                let mut s = settings_always_short();
                                s.sizing = Sizing::FixedQty { qty };
                                s.spread_pct = spread;
                                s.fees = Fees { amount_kind: fk.into(), per: fp.into(), amount: fa };
                                s.pyramiding = pyr;
                                {
                                    let sd = s.short.as_mut().unwrap();
                                    sd.stop_loss_pct = sl;
                                    sd.take_profit_pct = tp;
                                }
                                short_cases.push((
                                    format!("SHORT_qty{qty}_sp{spread}_{fk}-{fp}-{fa}_sl{sl}_tp{tp}_pyr{pyr}"),
                                    s,
                                ));
                            }
                        }
                    }
                }
            }
        }
        for &slip in &[0.002f64] {
            let mut s = settings_always_short();
            s.sizing = Sizing::FixedQty { qty: 2.0 };
            s.slippage = Slippage { kind: "pct".into(), value: slip, tick_size: 0.0 };
            s.short.as_mut().unwrap().stop_loss_pct = 0.03;
            s.short.as_mut().unwrap().take_profit_pct = 0.05;
            short_cases.push((format!("SHORT_slip{slip}"), s));
        }
        for &pct in &[10.0f64] {
            let mut s = settings_always_short();
            s.sizing = Sizing::PercentEquity { percent: pct };
            s.short.as_mut().unwrap().stop_loss_pct = 0.03;
            s.short.as_mut().unwrap().take_profit_pct = 0.06;
            short_cases.push((format!("SHORT_pctEq{pct}"), s));
        }
        cases.extend(short_cases);

        println!("###CASES### {}", cases.len());
        for (name, s) in &cases {
            let r = run(s, &b);
            let out = serde_json::json!({
                "name": name,
                "final_equity": r.stats.final_equity,
                "net_pnl": r.stats.net_pnl,
                "trades": r.trades.iter().map(|t| serde_json::json!({
                    "entry_price": t.entry_price,
                    "exit_price": t.exit_price,
                    "qty": t.qty,
                    "entries": t.entries,
                    "reason": t.exit_reason,
                    "pnl": t.pnl,
                    "fees": t.fees,
                })).collect::<Vec<_>>(),
                "cfg": {
                    "sizing": match &s.sizing {
                        Sizing::FixedQty { qty } => serde_json::json!({"mode":"fixed_qty","qty":qty}),
                        Sizing::PercentEquity { percent } => serde_json::json!({"mode":"percent_equity","percent":percent}),
                        Sizing::Risk { risk_pct } => serde_json::json!({"mode":"risk","risk_pct":risk_pct}),
                        _ => serde_json::json!({"mode":"other"}),
                    },
                    "spread_pct": s.spread_pct,
                    "fees": {"amount_kind": s.fees.amount_kind, "per": s.fees.per, "amount": s.fees.amount},
                    "pyramiding": s.pyramiding,
                    "leverage": s.leverage,
                    "starting_capital": s.starting_capital,
                    "instrument": {"multiplier": s.instrument.multiplier, "lot_step": s.instrument.lot_step, "min_qty": s.instrument.min_qty},
                    "slippage": {"kind": s.slippage.kind, "value": s.slippage.value, "tick_size": s.slippage.tick_size},
                    "side": if s.short.is_some() { "short" } else { "long" },
                    "long": {
                        "stop_loss_pct": s.long.as_ref().map(|x| x.stop_loss_pct).unwrap_or(0.0),
                        "take_profit_pct": s.long.as_ref().map(|x| x.take_profit_pct).unwrap_or(0.0),
                        "exit_on_reverse": s.long.as_ref().map(|x| x.exit_on_reverse).unwrap_or(false),
                        "trailing_stop": s.long.as_ref().and_then(|x| x.trailing_stop.as_ref()).map(trail_json),
                    },
                    "short": {
                        "stop_loss_pct": s.short.as_ref().map(|x| x.stop_loss_pct).unwrap_or(0.0),
                        "take_profit_pct": s.short.as_ref().map(|x| x.take_profit_pct).unwrap_or(0.0),
                        "exit_on_reverse": s.short.as_ref().map(|x| x.exit_on_reverse).unwrap_or(false),
                        "trailing_stop": s.short.as_ref().and_then(|x| x.trailing_stop.as_ref()).map(trail_json),
                    },
                },
            });
            println!("###CASE### {}", serde_json::to_string(&out).unwrap());
        }
    }
}

/// Trailing stop: the level is recomputed only on a **closed** candle, the exit is the same
/// intrabar touch as the fixed stop, and it never loosens.
#[cfg(test)]
mod trailing {
    use super::tests::*;
    use super::*;

    fn approx_v(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() < tol, "{what}: expected {b}, got {a} (diff {})", (a - b).abs());
    }
    fn ts_of(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2024-01-{:02}T00:00:00Z", i + 1)).collect()
    }
    fn bars<'a>(ts: &'a [String], o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> Bars<'a> {
        Bars { ticker: "X", ts, open: o, high: h, low: l, close: c, volume: c }
    }
    fn trail(kind: &str, value: f64) -> Trail {
        Trail { kind: kind.into(), value, ..Trail::default() }
    }
    /// `settings_always_long` with a trailing stop on the enabled side.
    fn with_trail(t: Trail, short: bool) -> Settings {
        let mut s = if short { settings_always_short() } else { settings_always_long() };
        let side = if short { s.short.as_mut() } else { s.long.as_mut() };
        side.unwrap().trailing_stop = Some(t);
        s
    }

    /// The reference case. Entry at 100, 10% trail. The level ratchets at each close
    /// (110 → 99, then 120 → 108) and the third bar's low of 100 takes it out at 108.
    #[test]
    fn long_ratchets_on_close_and_exits_intrabar() {
        let ts = ts_of(6);
        let o = [100.0, 100.0, 110.0, 120.0, 120.0, 120.0];
        let h = [100.0, 110.0, 120.0, 125.0, 125.0, 125.0];
        let l = [100.0, 100.0, 105.0, 100.0, 90.0, 90.0];
        let c = [100.0, 110.0, 115.0, 120.0, 100.0, 100.0];
        let r = run(&with_trail(trail("pct", 0.10), false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "trailing_stop", "{t:?}");
        assert_eq!(t.entry_ts, ts[1], "entry on the bar after the first signal");
        assert_eq!(t.exit_ts, ts[3], "the level from bar 2's close is what bar 3 reaches");
        approx_v(t.exit_price, 108.0, 1e-9, "120 high − 10%");
        approx_v(t.entry_price, 100.0, 1e-9, "entry at bar 1 open");
    }

    /// The same, short: the level ratchets down and never back up.
    #[test]
    fn short_ratchets_on_close_and_exits_intrabar() {
        let ts = ts_of(6);
        let o = [100.0, 100.0, 90.0, 80.0, 80.0, 80.0];
        let h = [100.0, 100.0, 95.0, 100.0, 110.0, 110.0];
        let l = [100.0, 90.0, 80.0, 75.0, 75.0, 75.0];
        let c = [100.0, 90.0, 85.0, 80.0, 100.0, 100.0];
        let r = run(&with_trail(trail("pct", 0.10), true), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "trailing_stop", "{t:?}");
        assert_eq!(t.direction, "short");
        assert_eq!(t.exit_ts, ts[3]);
        approx_v(t.exit_price, 88.0, 1e-9, "80 low + 10%");
    }

    /// The property the whole design turns on: the level a bar is tested against was computed
    /// **before** that bar. Bar 2 spikes to 200 and dips to 95. A level derived from bar 2's own
    /// high (180) would have been touched by bar 2's own low and closed the trade there. Only
    /// bar 3 may see 180.
    #[test]
    fn level_never_comes_from_the_bar_it_triggers_on() {
        let ts = ts_of(5);
        let o = [100.0, 100.0, 100.0, 190.0, 190.0];
        let h = [100.0, 100.0, 200.0, 195.0, 195.0];
        let l = [100.0, 100.0, 95.0, 170.0, 170.0];
        let c = [100.0, 100.0, 190.0, 175.0, 175.0];
        let r = run(&with_trail(trail("pct", 0.10), false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_ts, ts[3], "bar 2's low of 95 must not touch bar 2's own 180 level");
        approx_v(t.exit_price, 180.0, 1e-9, "200 high − 10%, seen only once bar 2 closed");
        assert_eq!(t.exit_reason, "trailing_stop");
    }

    /// A gap through the trailing level fills at the open, not at the level: the same rule the
    /// fixed stop already applies, one code path.
    #[test]
    fn gap_through_the_level_fills_at_the_open() {
        let ts = ts_of(5);
        let o = [100.0, 100.0, 110.0, 80.0, 80.0];
        let h = [100.0, 110.0, 115.0, 85.0, 85.0];
        let l = [100.0, 100.0, 108.0, 70.0, 70.0];
        let c = [100.0, 110.0, 112.0, 75.0, 75.0];
        let r = run(&with_trail(trail("pct", 0.10), false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "trailing_stop");
        // Level after bar 2's close: 115 − 11.5 = 103.5. Bar 3 opens at 80, far below it.
        approx_v(t.exit_price, 80.0, 1e-9, "no liquidity at 103.5, the open is the first price");
    }

    /// `activate_pct` holds the trail off the book until the trade is that far in profit: a 20%
    /// adverse excursion before the threshold does not close anything.
    #[test]
    fn activation_threshold_arms_the_trail_late() {
        let ts = ts_of(6);
        let o = [100.0, 100.0, 100.0, 100.0, 125.0, 125.0];
        let h = [100.0, 105.0, 100.0, 130.0, 125.0, 125.0];
        let l = [100.0, 95.0, 80.0, 95.0, 110.0, 110.0];
        let c = [100.0, 100.0, 90.0, 125.0, 115.0, 115.0];
        let t20 = Trail { activate_pct: 0.20, ..trail("pct", 0.10) };
        let r = run(&with_trail(t20, false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_ts, ts[4], "bar 2's drop to 80 came before the trail armed");
        approx_v(t.exit_price, 117.0, 1e-9, "armed at bar 3 (+30%): 130 − 10%");
        assert_eq!(t.exit_reason, "trailing_stop");

        // Without the threshold the same bars close on bar 2 instead, at 100.5 − the level the
        // entry bar's 105 high had already set.
        let r0 = run(&with_trail(trail("pct", 0.10), false), &bars(&ts, &o, &h, &l, &c));
        assert_eq!(r0.trades[0].exit_ts, ts[2], "no threshold ⇒ live from the entry bar");
        approx_v(r0.trades[0].exit_price, 94.5, 1e-9, "105 − 10%, gapped to bar 2's open of 100");
    }

    /// A breakeven step with no trailing distance is a legal plan on its own: the stop goes to
    /// the average entry and stays there.
    #[test]
    fn breakeven_step_without_a_trail_distance() {
        let ts = ts_of(5);
        let o = [100.0, 100.0, 102.0, 102.0, 102.0];
        let h = [100.0, 106.0, 103.0, 103.0, 103.0];
        let l = [100.0, 100.0, 95.0, 95.0, 95.0];
        let c = [100.0, 105.0, 100.0, 100.0, 100.0];
        let be = Trail { breakeven_pct: 0.05, ..Trail::default() };
        let r = run(&with_trail(be, false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "trailing_stop");
        assert_eq!(t.exit_ts, ts[2]);
        approx_v(t.exit_price, 100.0, 1e-9, "the average entry, reached at +6% on bar 1");
        approx_v(t.pnl, 0.0, 1e-9, "gross breakeven, fees excluded");
    }

    /// Breakeven and trail are independent steps and both only tighten: at +6% the stop is the
    /// entry (100), which is *above* what the 20% trail would give (106 − 21.2), so the entry
    /// wins; the trail takes over later once it has climbed past it.
    #[test]
    fn breakeven_and_trail_take_whichever_is_tighter() {
        let ts = ts_of(6);
        let o = [100.0, 100.0, 104.0, 130.0, 130.0, 130.0];
        let h = [100.0, 106.0, 140.0, 135.0, 135.0, 135.0];
        let l = [100.0, 100.0, 103.0, 110.0, 110.0, 110.0];
        let c = [100.0, 105.0, 138.0, 120.0, 120.0, 120.0];
        let t = Trail { breakeven_pct: 0.05, ..trail("pct", 0.20) };
        let r = run(&with_trail(t, false), &bars(&ts, &o, &h, &l, &c));
        let tr = &r.trades[0];
        assert_eq!(tr.exit_ts, ts[3], "bar 2's low of 103 sits above the breakeven stop at 100");
        approx_v(tr.exit_price, 112.0, 1e-9, "bar 2's 140 high − 20%");
    }

    /// A fixed stop and a trail on the same side: the tighter level is the one the market
    /// reaches first, and it names the exit.
    #[test]
    fn fixed_stop_and_trail_compete_and_the_reason_says_which() {
        let ts = ts_of(6);
        // The trail climbs above the fixed 90 and closes the trade.
        let o = [100.0, 100.0, 110.0, 120.0, 120.0, 120.0];
        let h = [100.0, 110.0, 120.0, 125.0, 125.0, 125.0];
        let l = [100.0, 100.0, 105.0, 100.0, 90.0, 90.0];
        let c = [100.0, 110.0, 115.0, 120.0, 100.0, 100.0];
        let mut s = with_trail(trail("pct", 0.10), false);
        s.long.as_mut().unwrap().stop_loss = Some(Stop { kind: "pct".into(), value: 0.10, period: 0 });
        let r = run(&s, &bars(&ts, &o, &h, &l, &c));
        assert_eq!(r.trades[0].exit_reason, "trailing_stop");
        approx_v(r.trades[0].exit_price, 108.0, 1e-9, "trail 108 is tighter than the fixed 90");

        // Straight down from entry: the trail never gets above the fixed stop, so the fixed one
        // owns the exit and the reason stays "stop_loss".
        let o2 = [100.0, 100.0, 95.0, 95.0, 95.0, 95.0];
        let h2 = [100.0, 100.0, 96.0, 96.0, 96.0, 96.0];
        let l2 = [100.0, 92.0, 85.0, 85.0, 85.0, 85.0];
        let c2 = [100.0, 95.0, 88.0, 88.0, 88.0, 88.0];
        let r2 = run(&s, &bars(&ts, &o2, &h2, &l2, &c2));
        assert_eq!(r2.trades[0].exit_reason, "stop_loss", "a tie goes to the fixed stop");
        approx_v(r2.trades[0].exit_price, 90.0, 1e-9, "the fixed 10% level");
    }

    /// The whole distance vocabulary: a percent follows the anchor, an absolute distance ignores
    /// it, and 0 disables. There is no third kind.
    #[test]
    fn distance_by_kind() {
        approx_v(trail("pct", 0.10).distance(200.0).unwrap(), 20.0, 1e-9, "10% of 200");
        approx_v(trail("pct", 0.10).distance(50.0).unwrap(), 5.0, 1e-9, "10% of 50");
        approx_v(trail("abs", 2.5).distance(200.0).unwrap(), 2.5, 1e-9, "abs ignores anchor");
        approx_v(trail("abs", 2.5).distance(50.0).unwrap(), 2.5, 1e-9, "abs ignores anchor");
        assert!(trail("pct", 0.0).distance(100.0).is_none(), "0 disables");
    }

    /// An ATR trail is an indicator wearing a stop's clothes, and a silent one: its distance was
    /// unresolvable for any trade opened inside the ATR warm-up, so the stop simply was not
    /// there. The rule is refused now, loudly, and a saved strategy carrying one says so.
    #[test]
    fn atr_is_not_a_trail_kind() {
        let s = with_trail(trail("atr", 3.0), false);
        let err = s.validate().expect("an ATR trail is refused");
        assert!(err.contains("indicator"), "{err}");

        let json = serde_json::json!({ "kind": "atr", "value": 3.0, "period": 14 });
        let t: Trail = serde_json::from_value(json).unwrap();
        assert_eq!(t.kind, "atr", "an old strategy still deserializes");
        assert!(t.validate().is_some(), "and is refused at validation, not silently ignored");
    }

    /// An `abs` trail holds a constant price distance behind the high.
    #[test]
    fn absolute_distance_trail() {
        let ts = ts_of(5);
        let o = [100.0, 100.0, 110.0, 118.0, 118.0];
        let h = [100.0, 110.0, 120.0, 121.0, 121.0];
        let l = [100.0, 100.0, 109.0, 114.0, 114.0];
        let c = [100.0, 110.0, 119.0, 115.0, 115.0];
        let r = run(&with_trail(trail("abs", 5.0), false), &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.exit_ts, ts[3]);
        approx_v(t.exit_price, 115.0, 1e-9, "120 high − 5");
    }

    /// Regression guard: a trail that does nothing must change nothing. An absent rule, a rule
    /// at zero and a rule whose threshold is never reached all produce the same run.
    #[test]
    fn an_inert_trail_is_a_no_op() {
        let ts = ts_of(8);
        let o = [100.0, 101.0, 103.0, 99.0, 97.0, 102.0, 104.0, 101.0];
        let h = [102.0, 104.0, 105.0, 101.0, 100.0, 105.0, 106.0, 103.0];
        let l = [99.0, 100.0, 98.0, 96.0, 95.0, 99.0, 100.0, 98.0];
        let c = [101.0, 103.0, 99.0, 97.0, 99.0, 104.0, 101.0, 102.0];
        let b = bars(&ts, &o, &h, &l, &c);
        let mut base = settings_always_long();
        base.long.as_mut().unwrap().stop_loss = Some(Stop { kind: "pct".into(), value: 0.05, period: 0 });
        base.long.as_mut().unwrap().take_profit = Some(Stop { kind: "pct".into(), value: 0.04, period: 0 });
        let want = run(&base, &b);

        let sig = |r: &RunResult| {
            r.trades
                .iter()
                .map(|t| (t.entry_ts.clone(), t.exit_ts.clone(), t.exit_price, t.pnl, t.exit_reason.clone()))
                .collect::<Vec<_>>()
        };
        for (what, t) in [
            ("zeroed rule", Trail::default()),
            ("unreachable threshold", Trail { activate_pct: 9.0, ..trail("pct", 0.10) }),
        ] {
            let mut s = base.clone();
            s.long.as_mut().unwrap().trailing_stop = Some(t);
            let got = run(&s, &b);
            assert_eq!(sig(&got), sig(&want), "{what} changed the run");
            approx_v(got.stats.net_pnl, want.stats.net_pnl, 1e-9, what);
        }
    }

    /// Risk-based sizing needs a stop price *at entry*. A trail that is live from the entry bar
    /// is one; a trail waiting on an activation threshold, or a bare breakeven step, is not.
    #[test]
    fn risk_sizing_accepts_only_a_trail_that_exists_at_entry() {
        let mut s = settings_always_long();
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss = None;
        s.long.as_mut().unwrap().stop_loss_pct = 0.0;
        assert!(s.validate().is_some(), "no stop at all is refused");

        s.long.as_mut().unwrap().trailing_stop = Some(trail("pct", 0.05));
        assert!(s.validate().is_none(), "a trail live from entry sizes a risk");

        s.long.as_mut().unwrap().trailing_stop = Some(Trail { activate_pct: 0.02, ..trail("pct", 0.05) });
        assert!(s.validate().is_some(), "a trail that arms later is no stop at entry");

        s.long.as_mut().unwrap().trailing_stop = Some(Trail { breakeven_pct: 0.02, ..Trail::default() });
        assert!(s.validate().is_some(), "a breakeven step alone is no stop at entry");
    }

    /// Risk sizing actually reads the trail's entry level: 1% of 10_000 risked over a 5% stop
    /// on a 100 entry is 100/5 = 20 units.
    #[test]
    fn risk_sizing_sizes_off_the_trail_level() {
        let ts = ts_of(4);
        let o = [100.0, 100.0, 100.0, 100.0];
        let h = [100.0, 100.0, 100.0, 100.0];
        let l = [100.0, 100.0, 100.0, 100.0];
        let c = [100.0, 100.0, 100.0, 100.0];
        let mut s = with_trail(trail("pct", 0.05), false);
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.starting_capital = 10_000.0;
        let r = run(&s, &bars(&ts, &o, &h, &l, &c));
        approx_v(r.trades[0].qty, 20.0, 1e-9, "100 risked / 5 of stop distance");
    }

    /// The general no-lookahead property, stated the way a skeptic would want it: what the engine
    /// decided by bar k must not depend on bars after k. Running the same settings over a prefix
    /// of the data has to reproduce, trade for trade, everything the full run closed before that
    /// prefix ended. If the trail peeked even one bar ahead, the prefix runs would disagree.
    #[test]
    fn a_prefix_of_the_data_reproduces_the_same_closed_trades() {
        let n = 600;
        let (o, h, l, c) = {
            let mut st = 99u64;
            let mut next = || {
                st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                ((st >> 32) as f64) / ((1u64 << 31) as f64) - 1.0 // symmetric [-1, 1)
            };
            let (mut o, mut h, mut l, mut c) = (vec![], vec![], vec![], vec![]);
            let (mut px, mut mom) = (100.0f64, 0.0f64);
            for _ in 0..n {
                mom = mom * 0.85 + next() * 1.0;
                let open = px;
                let close = (px * (1.0 + mom / 100.0) + (100.0 - px) * 0.02).max(1.0);
                h.push(open.max(close) * (1.0 + next().abs() * 0.008));
                l.push(open.min(close) * (1.0 - next().abs() * 0.008));
                o.push(open);
                c.push(close);
                px = close;
            }
            (o, h, l, c)
        };
        let ts: Vec<String> =
            (0..n).map(|i| format!("2024-01-01T{:02}:{:02}:00Z", i / 60, i % 60)).collect();

        let key = |t: &Trade| {
            (
                t.entry_ts.clone(),
                t.exit_ts.clone(),
                format!("{:.9}", t.entry_price),
                format!("{:.9}", t.exit_price),
                format!("{:.9}", t.pnl),
                t.exit_reason.clone(),
            )
        };

        let mut compared = 0usize;
        for short in [false, true] {
            for &(act, be) in &[(0.0f64, 0.0f64), (0.04, 0.0), (0.0, 0.02)] {
                let tr = Trail {
                    kind: "pct".into(),
                    value: 0.03,
                    activate_pct: act,
                    breakeven_pct: be,
                };
                let mut s = with_trail(tr, short);
                s.sizing = Sizing::FixedQty { qty: 1.0 };
                let full = run(&s, &bars(&ts, &o, &h, &l, &c));

                for k in [137usize, 288, 451] {
                    let part = run(
                        &s,
                        &bars(&ts[..k], &o[..k], &h[..k], &l[..k], &c[..k]),
                    );
                    // The prefix run force-closes on its own last bar, so compare only the
                    // trades that had already closed before it.
                    let cutoff = &ts[k - 1];
                    let want: Vec<_> =
                        full.trades.iter().filter(|t| &t.exit_ts < cutoff).map(key).collect();
                    let got: Vec<_> =
                        part.trades.iter().filter(|t| &t.exit_ts < cutoff).map(key).collect();
                    assert_eq!(got, want, "short={short} act={act} be={be} prefix={k}");
                    compared += want.len();
                }
            }
        }
        assert!(compared > 100, "the prefixes must actually contain trades, got {compared}");
    }

    /// Every trailing exit has to be a price that could actually be traded. Over a long random
    /// path and a sweep of configurations: the fill sits inside the exit bar's range, it is never
    /// better than that bar's open (a stop is a market order, a gap does not fill at the level),
    /// and it is never tighter than the level the anchor allows. Those are the three ways a stop
    /// backtest flatters itself.
    #[test]
    fn every_trailing_fill_is_a_price_that_traded() {
        let n = 400;
        let (o, h, l, c) = {
            let mut st = 7u64;
            let mut next = || {
                st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                ((st >> 32) as f64) / ((1u64 << 31) as f64) - 1.0 // symmetric [-1, 1)
            };
            let (mut o, mut h, mut l, mut c) = (vec![], vec![], vec![], vec![]);
            let mut px = 100.0f64;
            for _ in 0..n {
                let open = px;
                let close = (px + next() * 2.5).max(1.0);
                h.push(open.max(close) + next().abs() * 2.0);
                l.push((open.min(close) - next().abs() * 2.0).max(0.5));
                o.push(open);
                c.push(close);
                px = close;
            }
            (o, h, l, c)
        };
        // One row per minute: `ts_of` only stays chronological while the day stays two digits,
        // and the engine orders its rows by the timestamp string.
        let ts: Vec<String> =
            (0..n).map(|i| format!("2024-01-01T{:02}:{:02}:00Z", i / 60, i % 60)).collect();
        let b = bars(&ts, &o, &h, &l, &c);
        let idx: std::collections::HashMap<&str, usize> =
            ts.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();

        let mut checked = 0usize;
        for short in [false, true] {
            for &(kind, value) in &[("pct", 0.02f64), ("pct", 0.08), ("abs", 1.0), ("abs", 5.0)] {
                for &act in &[0.0f64, 0.05] {
                    for &be in &[0.0f64, 0.03] {
                        let tr = Trail {
                            kind: kind.into(),
                            value,
                            activate_pct: act,
                            breakeven_pct: be,
                        };
                        let mut s = with_trail(tr, short);
                        s.sizing = Sizing::FixedQty { qty: 1.0 };
                        let r = run(&s, &b);
                        for t in &r.trades {
                            if t.exit_reason != "trailing_stop" {
                                continue;
                            }
                            let (ei, xi) = (idx[t.entry_ts.as_str()], idx[t.exit_ts.as_str()]);
                            let px = t.exit_price;
                            assert!(
                                px >= l[xi] - 1e-9 && px <= h[xi] + 1e-9,
                                "fill {px} outside bar {xi} range [{}, {}]",
                                l[xi],
                                h[xi]
                            );
                            // The level the trail could have reached, from the best price seen
                            // while the trade was open (the exit bar is included, which only
                            // loosens the bound, never tightens it into a false failure). The
                            // breakeven step can sit tighter than the trail, so it joins the cap.
                            let (best, d) = if short {
                                let mut best = l[ei..=xi].iter().copied().fold(f64::INFINITY, f64::min);
                                // With no activation threshold the anchor is floored at the
                                // average entry, so a trade that never traded better than its
                                // entry still has a level to be stopped on.
                                if act <= 0.0 {
                                    best = best.min(t.entry_price);
                                }
                                (best, if kind == "abs" { value } else { best * value })
                            } else {
                                let mut best = h[ei..=xi].iter().copied().fold(f64::NEG_INFINITY, f64::max);
                                if act <= 0.0 {
                                    best = best.max(t.entry_price);
                                }
                                (best, if kind == "abs" { value } else { best * value })
                            };
                            if short {
                                let cap =
                                    if be > 0.0 { (best + d).min(t.entry_price) } else { best + d };
                                assert!(px >= cap - 1e-6, "short fill {px} tighter than its level {cap}");
                                assert!(px >= o[xi] - 1e-9, "short fill {px} better than the open {}", o[xi]);
                            } else {
                                let cap =
                                    if be > 0.0 { (best - d).max(t.entry_price) } else { best - d };
                                assert!(px <= cap + 1e-6, "long fill {px} tighter than its level {cap}");
                                assert!(px <= o[xi] + 1e-9, "long fill {px} better than the open {}", o[xi]);
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 500, "the sweep must actually produce trailing exits, got {checked}");
    }

    /// The trail follows the *average* entry when pyramiding moves it, and the level still only
    /// tightens.
    #[test]
    fn pyramiding_moves_the_breakeven_anchor_to_the_new_average() {
        let ts = ts_of(6);
        let o = [100.0, 100.0, 120.0, 130.0, 130.0, 130.0];
        let h = [100.0, 105.0, 125.0, 135.0, 135.0, 135.0];
        let l = [100.0, 100.0, 119.0, 105.0, 105.0, 105.0];
        let c = [100.0, 104.0, 124.0, 110.0, 110.0, 110.0];
        let mut s = with_trail(Trail { breakeven_pct: 0.0001, ..Trail::default() }, false);
        s.pyramiding = 2;
        let r = run(&s, &bars(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        assert_eq!(t.entries, 2, "entered at 100 then added at 120");
        approx_v(t.entry_price, 110.0, 1e-9, "average of 100 and 120");
        assert_eq!(t.exit_reason, "trailing_stop");
        approx_v(t.exit_price, 110.0, 1e-9, "breakeven on the average, not on the first lot");
    }
}

/// A real strategy, run end to end, so the trailing stop can be read as a trader would read it:
/// does the money behave the way a trailing stop is supposed to behave? Printed, not asserted on
/// the numbers themselves (a random path is not a benchmark) — what IS asserted is the economics
/// that must hold on any path: a trail caps the give-back from the peak, and it cannot invent
/// return out of thin air. `cargo test -p otw-core --bin otw-core backtest::strategy_returns
/// -- --ignored --nocapture`
#[cfg(test)]
mod strategy_returns {
    use super::tests::*;
    use super::*;

    /// A trending random walk (momentum + mean reversion), so an EMA crossover has something
    /// real to catch instead of pure noise.
    fn path(n: usize, seed: u64) -> (Vec<String>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let mut st = seed;
        let mut next = || {
            st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((st >> 33) as f64) / ((1u64 << 31) as f64) - 1.0
        };
        let (mut o, mut h, mut l, mut c) = (vec![], vec![], vec![], vec![]);
        let mut px = 100.0f64;
        let mut mom = 0.0f64;
        for _ in 0..n {
            // Momentum decays and is re-kicked: trends that run, then turn.
            mom = mom * 0.85 + next() * 1.0;
            let open = px;
            // A pull back to 100 keeps a 3000-bar walk inside a tradable band instead of drifting
            // to zero or to the moon. The result runs at about 1% a bar, so a 3% stop is a
            // distance the path actually reaches.
            let close = (px * (1.0 + mom / 100.0) + (100.0 - px) * 0.02).max(1.0);
            h.push(open.max(close) * (1.0 + next().abs() * 0.008));
            l.push(open.min(close) * (1.0 - next().abs() * 0.008));
            o.push(open);
            c.push(close);
            px = close;
        }
        let ts = (0..n).map(|i| format!("2024-01-01T{:02}:{:02}:00Z", i / 60, i % 60)).collect();
        (ts, o, h, l, c)
    }

    fn ema_cross(fast: usize, slow: usize, up: bool) -> SignalGroup {
        SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Indicator {
                    indicator: "ema".into(),
                    period: fast,
                    fast: 0,
                    slow: 0,
                    mult: 0.0,
                    signal_period: 0,
                },
                op: if up { Op::CrossesAbove } else { Op::CrossesBelow },
                right: Some(Operand::Indicator {
                    indicator: "ema".into(),
                    period: slow,
                    fast: 0,
                    slow: 0,
                    mult: 0.0,
                    signal_period: 0,
                }),
            }],
        }
    }

    fn strategy() -> Settings {
        let mut s = settings_always_long();
        s.sizing = Sizing::PercentEquity { percent: 95.0 };
        s.starting_capital = 10_000.0;
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 0.05 };
        s.spread_pct = 0.0004;
        {
            let sd = s.long.as_mut().unwrap();
            sd.entry = Some(ema_cross(12, 48, true));
            sd.exit = Some(ema_cross(12, 48, false));
            sd.signal = None;
        }
        s
    }

    #[test]
    #[ignore]
    fn report() {
        let n = 3000;
        let (ts, o, h, l, c) = path(n, 20240115);
        let b = Bars { ticker: "X", ts: &ts, open: &o, high: &h, low: &l, close: &c, volume: &c };
        let buy_hold = (c[n - 1] / o[0] - 1.0) * 100.0;

        let variants: Vec<(&str, Box<dyn Fn(&mut Side)>)> = vec![
            ("signals only", Box::new(|_: &mut Side| {})),
            ("fixed SL 3%", Box::new(|sd: &mut Side| sd.stop_loss_pct = 0.03)),
            (
                "trail 3%",
                Box::new(|sd: &mut Side| {
                    sd.trailing_stop = Some(Trail { kind: "pct".into(), value: 0.03, ..Trail::default() })
                }),
            ),
            (
                "trail 3% + BE 2%",
                Box::new(|sd: &mut Side| {
                    sd.trailing_stop = Some(Trail {
                        kind: "pct".into(),
                        value: 0.03,
                        breakeven_pct: 0.02,
                        ..Trail::default()
                    })
                }),
            ),
            (
                "trail 3% from +2%",
                Box::new(|sd: &mut Side| {
                    sd.trailing_stop = Some(Trail {
                        kind: "pct".into(),
                        value: 0.03,
                        activate_pct: 0.02,
                        ..Trail::default()
                    })
                }),
            ),
            (
                "SL 3% + trail 5%",
                Box::new(|sd: &mut Side| {
                    sd.stop_loss_pct = 0.03;
                    sd.trailing_stop = Some(Trail { kind: "pct".into(), value: 0.05, ..Trail::default() })
                }),
            ),
        ];

        println!("\nbuy & hold over the path: {buy_hold:+.2}%   ({n} bars)");
        println!(
            "{:<20} {:>8} {:>7} {:>8} {:>9} {:>9} {:>8} {:>7} {:>28}",
            "variant", "return", "trades", "win%", "avg win", "avg loss", "maxDD", "bars", "exits"
        );
        for (name, tweak) in &variants {
            let mut s = strategy();
            tweak(s.long.as_mut().unwrap());
            let r = run(&s, &b);
            let st = &r.stats;
            let wins: Vec<f64> = r.trades.iter().map(|t| t.pnl).filter(|p| *p > 0.0).collect();
            let losses: Vec<f64> = r.trades.iter().map(|t| t.pnl).filter(|p| *p <= 0.0).collect();
            let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
            let mut reasons: std::collections::BTreeMap<&str, usize> = Default::default();
            for t in &r.trades {
                *reasons.entry(t.exit_reason.as_str()).or_default() += 1;
            }
            let exits = reasons.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" ");
            println!(
                "{name:<20} {:>7.2}% {:>7} {:>7.1} {:>9.1} {:>9.1} {:>7.2}% {:>7.1} {exits:>28}",
                st.return_pct,
                st.trades,
                st.win_rate,
                mean(&wins),
                mean(&losses),
                st.max_drawdown_pct,
                st.all.avg_bars_held,
            );

            // The economics that must hold whatever the path does. A trailing stop exists to cap
            // the give-back from the best price the trade saw; a trade it closed can never have
            // handed back more than the trail distance plus the gap that jumped it.
            if name.starts_with("trail 3%") {
                for t in r.trades.iter().filter(|t| t.exit_reason == "trailing_stop") {
                    let peak_equity_gain = t.mfe;
                    let given_back = peak_equity_gain - (t.pnl + t.fees);
                    let allowed = t.qty * t.entry_price * 0.03 * 1.5; // distance, plus gap slack
                    assert!(
                        given_back <= allowed + 1e-6,
                        "{name}: gave back {given_back:.2} of a {peak_equity_gain:.2} peak (cap {allowed:.2})"
                    );
                }
            }
        }
        println!();
    }
}
