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
pub mod holes;
pub(crate) mod indicators;
pub mod optimize;
pub mod params;
pub mod report;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
mod lookahead_tests;
#[cfg(test)]
mod hole_tests;
#[cfg(test)]
mod fuzz_tests;

pub use custom::CustomIndicatorDef;

use std::collections::{BTreeMap, HashMap};

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
///
/// v5: a position is tested against its stop and target on the bar it opens in (it used to be
/// tested from the next bar on); the take profit is a resting limit, so a bar that opens past it
/// fills there at the open, it fills at the target with no slippage and no spread charged, and it
/// triggers when the side that deals (the bid for a long) reaches it. New opt-in `execution`
/// settings add limit orders, bid/ask pricing and lower-timeframe resolution. A position closed
/// after a bar's open (stop, target, limit, close) no longer reopens at that same open. ATR stops
/// and targets take the ATR of the last closed bar, no longer that of the entry bar. Entries,
/// adds, reverses and circuit breakers at a bar's open are sized and gated on what is known at
/// that open (open prices, the positions held then), no longer on the bar's close.
///
/// v6 (lookahead audit, 2026-10-03): `exit_on_reverse` and an expired exit limit leave at the
/// next open instead of the close that decided them. A pyramid add is sent at the open before
/// the bar's stop and target are tested, and the bar is tested on the levels it made. A
/// position held at an open keeps its portfolio slot there. A row's assets act in the order
/// their sessions open. The trail on a limit's fill bar and MAE/MFE read only what the
/// position lived through. Grid: no-band ladders on the known range, buys only below the last
/// close, gap fills at the open, circuit stops at their level after the path, `session_end`
/// before the bar. The take profit may fill only through its level (`take_profit_fill`).
///
/// v7 (validation framework, 2026-10-04): a signal trade's entry fee leaves the cash at the fill,
/// as a broker debits it, so the equity of an open position, its drawdown and an add sized on it
/// are net of that fee (the trade's P&L is unchanged). `instrument.tick_size` puts every fill
/// and resting level on the tick grid, against the trader. A neutral grid trades long below its
/// centre line and short above it (it used to run as a long grid). Kelly sizing reads the
/// strategy's theoretical record (every signal trade, skipped ones included, run at the Kelly
/// cap) instead of the trades it took, so a negative window no longer stops it for good.
pub const ENGINE_VERSION: u32 = 7;

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
    /// A percent of notional when `amount_kind` is "pct" (0.1 = 0.1%), else a currency amount.
    /// Negative for a rebate. Charged at each fill; an entry's fee leaves the cash right away.
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
/// behavior); `atr` = a multiple of ATR(`period`) taken on the last bar closed before the first entry, held
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
    /// Exit when this side's entry group no longer holds: decided at that close, sent at the
    /// next open (a market order, or a limit resting from it).
    #[serde(default)]
    pub exit_on_reverse: bool,
}

impl Side {
    /// Effective stop-loss rule: the object form if present, else the legacy `stop_loss_pct`.
    pub(crate) fn sl_rule(&self) -> Option<Stop> {
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
    /// Price increment (0 = any price). Every fill and every resting level sits on this grid,
    /// rounded against the trader: a buy fills a tick up, a sell a tick down, a stop is placed
    /// where it costs more, a limit where it fills less easily.
    #[serde(default)]
    pub tick_size: f64,
}
fn default_one() -> f64 {
    1.0
}
impl Default for Instrument {
    fn default() -> Self {
        Instrument { multiplier: 1.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 }
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
/// `x` on the tick grid `tick`, rounded up (`up`) or down. A price already on the grid stays
/// (a relative tolerance absorbs the float noise of `x / tick`); `tick <= 0` leaves it as is.
pub(crate) fn on_tick(x: f64, tick: f64, up: bool) -> f64 {
    if !(tick > 0.0) || !x.is_finite() {
        return x;
    }
    let k = x / tick;
    let near = k.round();
    let k = if (k - near).abs() <= 1e-9 * near.abs().max(1.0) {
        near
    } else if up {
        k.ceil()
    } else {
        k.floor()
    };
    k * tick
}

impl Slippage {
    /// Tick size of the "ticks" mode (0 when unset: the instrument's tick then applies).
    pub(crate) fn tick(&self) -> f64 {
        self.tick_size
    }

    /// Absolute price slippage for a fill at `px` (always ≥ 0).
    pub(crate) fn amount(&self, px: f64) -> f64 {
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

/// How an order is sent: at market, or as a resting limit. A limit price sits `offset` away from
/// the reference (below it to buy, above it to sell) and rests for `valid_bars` bars.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct OrderSpec {
    /// "market" | "limit"
    #[serde(default = "default_order_kind")]
    pub kind: String,
    /// "pct" (fraction of the reference) | "abs" (price distance) | "atr" (ATR multiple).
    #[serde(default = "default_stop_kind")]
    pub offset_kind: String,
    /// Distance from the reference: a fraction (pct), a price (abs) or an ATR multiple (atr).
    #[serde(default)]
    pub offset: f64,
    /// ATR lookback for `offset_kind = "atr"` (defaults to 14).
    #[serde(default)]
    pub atr_period: usize,
    /// "close" (the close of the bar that gave the signal) | "open" (the open of the next bar).
    #[serde(default = "default_order_reference")]
    pub reference: String,
    /// Bars the order rests before it expires (1 = the bar after the signal only).
    #[serde(default = "default_one_bar")]
    pub valid_bars: usize,
    /// "touch" (fills when the price reaches the limit) | "through" (the price must trade
    /// beyond it, a stricter queue assumption).
    #[serde(default = "default_fill_rule")]
    pub fill: String,
    /// Exit orders only: what an unfilled exit limit does when it expires, "market" (closes at
    /// the open after its last bar) | "cancel" (the position stays open).
    #[serde(default = "default_on_expiry")]
    pub on_expiry: String,
}
fn default_order_kind() -> String {
    "market".into()
}
fn default_order_reference() -> String {
    "close".into()
}
fn default_one_bar() -> usize {
    1
}
fn default_fill_rule() -> String {
    "touch".into()
}
fn default_on_expiry() -> String {
    "market".into()
}
impl Default for OrderSpec {
    fn default() -> OrderSpec {
        OrderSpec {
            kind: default_order_kind(),
            offset_kind: default_stop_kind(),
            offset: 0.0,
            atr_period: 0,
            reference: default_order_reference(),
            valid_bars: 1,
            fill: default_fill_rule(),
            on_expiry: default_on_expiry(),
        }
    }
}
impl OrderSpec {
    fn is_limit(&self) -> bool {
        self.kind == "limit"
    }
    fn atr_period(&self) -> Option<usize> {
        (self.is_limit() && self.offset_kind == "atr")
            .then_some(if self.atr_period == 0 { 14 } else { self.atr_period })
    }
    /// Absolute price distance from `reference`, or None when an ATR offset has no value yet.
    fn distance(&self, reference: f64, atr: Option<f64>) -> Option<f64> {
        match self.offset_kind.as_str() {
            "abs" => Some(self.offset),
            "atr" => atr.filter(|a| *a > 0.0).map(|a| a * self.offset),
            _ => Some(reference.abs() * self.offset),
        }
    }
    fn validate(&self, label: &str, exit: bool) -> Option<String> {
        if !matches!(self.kind.as_str(), "market" | "limit") {
            return Some(format!("{label} order type must be market or limit (got '{}')", self.kind));
        }
        if !self.is_limit() {
            return None;
        }
        if !matches!(self.offset_kind.as_str(), "pct" | "abs" | "atr") {
            return Some(format!("{label} limit offset must be pct, abs or atr (got '{}')", self.offset_kind));
        }
        if self.offset < 0.0 || !self.offset.is_finite() {
            return Some(format!("{label} limit offset cannot be negative"));
        }
        if !matches!(self.reference.as_str(), "close" | "open") {
            return Some(format!("{label} limit reference must be close or open (got '{}')", self.reference));
        }
        if !matches!(self.fill.as_str(), "touch" | "through") {
            return Some(format!("{label} limit fill rule must be touch or through (got '{}')", self.fill));
        }
        if exit && !matches!(self.on_expiry.as_str(), "market" | "cancel") {
            return Some(format!("{label} limit expiry must be market or cancel (got '{}')", self.on_expiry));
        }
        None
    }
}

/// Order types and fill data. Every field defaults to the plain behaviour: market orders,
/// mid/trade bars with `spread_pct`, one timeframe, one fee rate.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Execution {
    /// How entries and pyramid adds are sent.
    #[serde(default)]
    pub entry_order: OrderSpec,
    /// How signal exits are sent (the exit condition and `exit_on_reverse`). A stop-and-reverse
    /// stays a market flip, the stop loss a stop order and the take profit a limit.
    #[serde(default)]
    pub exit_order: OrderSpec,
    /// Read a lower timeframe of the same instrument inside a bar where the order of events
    /// matters (stop and target both reached, a limit filled mid-bar).
    #[serde(default)]
    pub intrabar: bool,
    /// Lower timeframe to read ("1m", "5m", "15m", "1h", "4h"); empty = the finest one stored,
    /// else the finest the provider serves in one request per bar.
    #[serde(default)]
    pub intrabar_timeframe: String,
    /// Download from the provider the lower-timeframe candles a bar needs and the store lacks.
    /// A backtest asks first when that is long; a paper session does it on each tick.
    #[serde(default)]
    pub intrabar_fetch: bool,
    /// Price fills off stored bid/ask where the dataset has them (Capital.com, OANDA), instead
    /// of the mid shifted by half of `spread_pct`.
    #[serde(default)]
    pub use_quotes: bool,
    /// Fee for limit fills (entry and exit limits, take profit), in the unit of `fees.amount`.
    /// None = the same fee as market fills.
    #[serde(default)]
    pub maker_fee: Option<f64>,
    /// When the take profit, a resting limit, fills: "touch" (the price reaches it) | "through"
    /// (the price trades beyond it: a level touched exactly, at the top of the queue, is not
    /// enough).
    #[serde(default = "default_fill_rule")]
    pub take_profit_fill: String,
    /// Price on the stored bars as traded, without the split and dividend adjustment applied
    /// where the provider gives one.
    #[serde(default)]
    pub raw_prices: bool,
}
impl Default for Execution {
    fn default() -> Execution {
        Execution {
            entry_order: OrderSpec::default(),
            exit_order: OrderSpec::default(),
            intrabar: false,
            intrabar_timeframe: String::new(),
            intrabar_fetch: false,
            use_quotes: false,
            maker_fee: None,
            take_profit_fill: default_fill_rule(),
            raw_prices: false,
        }
    }
}

impl Execution {
    fn validate(&self) -> Option<String> {
        if let Some(e) = self.entry_order.validate("entry", false) {
            return Some(e);
        }
        if let Some(e) = self.exit_order.validate("exit", true) {
            return Some(e);
        }
        if !matches!(self.take_profit_fill.as_str(), "touch" | "through") {
            return Some(format!("take profit fill rule must be touch or through (got '{}')", self.take_profit_fill));
        }
        // A maker rebate is a negative fee: legal, only a non-number is refused.
        if self.maker_fee.is_some_and(|f| !f.is_finite()) {
            return Some("the maker fee must be a number".into());
        }
        if !matches!(self.intrabar_timeframe.as_str(), "" | "1m" | "5m" | "15m" | "1h" | "4h") {
            return Some(format!(
                "the lower timeframe must be 1m, 5m, 15m, 1h or 4h (got '{}')",
                self.intrabar_timeframe
            ));
        }
        None
    }
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
    /// "long" (buy dips, sell rallies) | "short" (mirror) | "neutral" (long in the cells below
    /// the centre line, short in the cells above it; needs an odd number of levels).
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
/// Most lines a ladder may have. Past this the cells are thinner than any real tick, and the
/// run cost grows with every bar times every cell.
pub const GRID_MAX_LEVELS: usize = 200;

impl GridConfig {
    fn anchored(&self) -> bool {
        !self.anchor.is_empty() && self.anchor != "none"
    }

    /// A ladder that cannot be traded is refused rather than reshaped in silence.
    fn validate(&self) -> Option<String> {
        let fin = |x: f64| x.is_finite();
        if !(2..=GRID_MAX_LEVELS).contains(&self.levels) {
            return Some(format!("a grid needs between 2 and {GRID_MAX_LEVELS} levels (got {})", self.levels));
        }
        if !matches!(self.direction.as_str(), "long" | "short" | "neutral") {
            return Some(format!("grid direction must be long, short or neutral (got '{}')", self.direction));
        }
        if self.direction == "neutral" && self.levels % 2 == 0 {
            return Some(format!(
                "a neutral grid trades long below its centre line and short above it: give it an odd number of levels (got {})",
                self.levels
            ));
        }
        if !(fin(self.qty_per_level) && self.qty_per_level >= 0.0) {
            return Some("the grid quantity per level cannot be negative (0 = use the budget)".into());
        }
        if !(fin(self.total_budget) && self.total_budget >= 0.0) {
            return Some("the grid budget cannot be negative (0 = use the starting capital)".into());
        }
        if !(fin(self.stop_below) && self.stop_below >= 0.0 && fin(self.stop_above) && self.stop_above >= 0.0) {
            return Some("grid stops cannot be negative (0 = none)".into());
        }
        if self.anchored() {
            if !matches!(self.anchor.as_str(), "sma" | "ema" | "dema" | "tema" | "wma" | "hma" | "vwap") {
                return Some(format!("unknown grid anchor '{}'", self.anchor));
            }
            if self.anchor_period == 0 {
                return Some("the grid anchor period must be at least 1".into());
            }
            if !matches!(self.width_kind.as_str(), "pct" | "atr") {
                return Some(format!("grid width must be pct or atr (got '{}')", self.width_kind));
            }
            if !(fin(self.width_value) && self.width_value > 0.0) {
                return Some("the grid half-width must be above 0".into());
            }
            if self.width_kind == "atr" && self.width_period == 0 {
                return Some("the grid ATR period must be at least 1".into());
            }
            return None;
        }
        // Both bounds 0: the ladder spans the range known so far (documented mode).
        if self.lower == 0.0 && self.upper == 0.0 {
            return None;
        }
        if !(fin(self.lower) && fin(self.upper)) {
            return Some("grid bounds must be numbers".into());
        }
        if self.lower >= self.upper {
            return Some(format!(
                "the grid lower bound must be below the upper bound (got {} and {})",
                self.lower, self.upper
            ));
        }
        None
    }
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
/// timestamp on the exchange/session clock the user works in: `timezone` when set (daylight
/// saving applied), else the fixed `tz_offset_min`.
///
/// Filters gate **new entries only**: exits, stops, take-profits, funding and marking-to-market
/// run on every bar, so a filter can never trap an open position. They also never trim bars:
/// indicators keep computing across a closed window, so switching a filter on doesn't shift an
/// EMA by one bar (that is the difference with narrowing the run's date window).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Filters {
    /// Minutes east of UTC for every rule here (+120 = UTC+2), used when `timezone` is empty.
    /// A fixed offset: no DST shift is applied.
    #[serde(default)]
    pub tz_offset_min: i32,
    /// IANA timezone ("America/New_York"). Wins over `tz_offset_min` and follows daylight
    /// saving, so a 09:30-16:00 New York session stays on the exchange's hours all year.
    #[serde(default)]
    pub timezone: String,
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
            timezone: String::new(),
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
        if !self.timezone.trim().is_empty() && time_tz::timezones::get_by_name(self.timezone.trim()).is_none() {
            return Some(format!("filters: unknown timezone \"{}\", use an IANA name like America/New_York", self.timezone));
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

    /// `dt` on the filters' clock: the timezone (with its daylight saving) when one is set,
    /// else the fixed offset.
    fn local(&self, dt: time::OffsetDateTime) -> time::OffsetDateTime {
        use time_tz::OffsetDateTimeExt;
        if let Some(tz) = time_tz::timezones::get_by_name(self.timezone.trim()) {
            return dt.to_timezone(tz);
        }
        let secs = self.tz_offset_min.clamp(-14 * 60, 14 * 60) * 60;
        dt.to_offset(time::UtcOffset::from_whole_seconds(secs).unwrap_or(time::UtcOffset::UTC))
    }

    /// May the strategy open on the bar stamped `ts`? An unparseable timestamp opens the gate:
    /// a clock rule is a restriction the user asked for on data it can read, never a silent halt.
    fn allows(&self, ts: &str) -> bool {
        use time::{format_description::well_known::Rfc3339, OffsetDateTime};
        let Ok(dt) = OffsetDateTime::parse(ts, &Rfc3339) else { return true };
        let local = self.local(dt);
        if !self.weekdays.is_empty()
            && !self.weekdays.contains(&local.weekday().number_from_monday())
        {
            return false;
        }
        if !self.date_allows(local.date()) {
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

    /// Whether `date` (already in the filters' local offset) is inside the calendar window.
    /// Date rules only: a weekday or session rule says when an entry may fire *inside* the
    /// window, it does not shorten the span the window covers.
    fn date_allows(&self, date: time::Date) -> bool {
        if !self.include_dates.is_empty() && !self.include_dates.iter().any(|r| r.contains(date)) {
            return false;
        }
        !self.exclude_dates.iter().any(|r| r.contains(date))
    }

    /// First and last row of `stamps` inside the calendar window, inclusive. `None` when no
    /// date rule is set: the window is then the whole series. An active rule matching no bar
    /// collapses to `(0, 0)`, which the callers read as "there was nothing to hold".
    fn date_window(&self, stamps: &[String]) -> Option<(usize, usize)> {
        use time::{format_description::well_known::Rfc3339, OffsetDateTime};
        if self.include_dates.is_empty() && self.exclude_dates.is_empty() {
            return None;
        }
        let (mut first, mut last) = (None, 0usize);
        for (i, t) in stamps.iter().enumerate() {
            let inside = OffsetDateTime::parse(t, &Rfc3339)
                .map(|dt| self.date_allows(self.local(dt).date()))
                .unwrap_or(true);
            if inside {
                first.get_or_insert(i);
                last = i;
            }
        }
        Some(first.map_or((0, 0), |f| (f, last)))
    }
}

/// Per-row "may open" flags for a clock, or None when the filters are inert (the hot path then
/// skips the lookup altogether). Computed once per run: the gate is a property of the clock,
/// not of an asset, so a portfolio run shares one verdict per row.
fn clock_gate(f: &Filters, clock: &[String]) -> Option<Vec<bool>> {
    if !f.is_active() {
        return None;
    }
    Some(clock.iter().map(|t| f.allows(t)).collect())
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
    /// Order types, bid/ask pricing and lower-timeframe resolution. Unset = market orders on
    /// the strategy's own bars.
    #[serde(default)]
    pub execution: Execution,
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
        if let Some(err) = self.validate_numbers() {
            return Some(err);
        }
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
        if let Some(err) = self.execution.validate() {
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

    /// Numbers that have no meaning, refused before a run instead of simulated into a result
    /// that looks like one: an account with no money, a size or a leverage of zero or below, a
    /// contract worth nothing. Fees may be negative either way: a maker rebate, a broker that
    /// pays per fill, are real.
    fn validate_numbers(&self) -> Option<String> {
        let fin = |x: f64| x.is_finite();
        if !fin(self.starting_capital) {
            return Some("starting capital must be a number".into());
        }
        if self.kind == "dca" {
            if self.starting_capital < 0.0 {
                return Some("starting capital cannot be negative".into());
            }
        } else if self.starting_capital <= 0.0 {
            return Some("starting capital must be above 0".into());
        }
        if !matches!(self.kind.as_str(), "signals" | "grid" | "dca") {
            return Some(format!("unknown strategy kind \"{}\" (signals|grid|dca)", self.kind));
        }
        if !(fin(self.leverage) && self.leverage > 0.0) {
            return Some("leverage must be above 0 (1 = no leverage)".into());
        }
        if !fin(self.fees.amount) {
            return Some("the fee must be a number".into());
        }
        if !matches!(self.fees.amount_kind.as_str(), "pct" | "fixed") {
            return Some(format!("fee kind must be pct or fixed (got '{}')", self.fees.amount_kind));
        }
        if !matches!(self.fees.per.as_str(), "trade" | "unit") {
            return Some(format!("fee basis must be trade or unit (got '{}')", self.fees.per));
        }
        if !(fin(self.spread_pct) && self.spread_pct >= 0.0) {
            return Some("the spread cannot be negative".into());
        }
        if !(fin(self.slippage.value) && self.slippage.value >= 0.0) {
            return Some("slippage cannot be negative".into());
        }
        // An empty kind is the derived default, read as "pct" by `Slippage::amount`.
        if !matches!(self.slippage.kind.as_str(), "" | "pct" | "ticks") {
            return Some(format!("slippage kind must be pct or ticks (got '{}')", self.slippage.kind));
        }
        if self.slippage.kind == "ticks"
            && self.slippage.value > 0.0
            && self.slippage.tick() <= 0.0
            && self.instrument.tick_size <= 0.0
        {
            return Some("slippage in ticks needs a tick size above 0".into());
        }
        let inst = &self.instrument;
        if !(fin(inst.multiplier) && inst.multiplier > 0.0) {
            return Some("the contract multiplier must be above 0".into());
        }
        for (v, what) in [(inst.lot_step, "lot step"), (inst.min_qty, "minimum quantity"), (inst.tick_size, "tick size")] {
            if !(fin(v) && v >= 0.0) {
                return Some(format!("the {what} cannot be negative (0 = none)"));
            }
        }
        if self.pyramiding == 0 {
            return Some("pyramiding must be at least 1 (1 = no pyramiding)".into());
        }
        let f = &self.funding;
        if !fin(f.annual_rate_pct) || (f.annual_rate_pct != 0.0 && !(fin(f.interval_hours) && f.interval_hours > 0.0)) {
            return Some("funding needs a rate and an interval above 0 hours".into());
        }
        if self.kind == "signals" {
            if let Some(err) = sizing_error(&self.sizing) {
                return Some(err);
            }
            for (label, side) in [("long", self.long.as_ref()), ("short", self.short.as_ref())] {
                let Some(sd) = side else { continue };
                if !(sd.stop_loss_pct >= 0.0 && sd.take_profit_pct >= 0.0) {
                    return Some(format!("{label}: stop loss and take profit cannot be negative"));
                }
                for st in [sd.stop_loss.as_ref(), sd.take_profit.as_ref()].into_iter().flatten() {
                    if !(fin(st.value) && st.value >= 0.0) || !matches!(st.kind.as_str(), "pct" | "atr") {
                        return Some(format!("{label}: a stop or target is a pct or atr distance of 0 or more"));
                    }
                }
            }
        }
        if self.kind == "grid" {
            // No `grid` at all is the documented known-range ladder (default config).
            if let Some(err) = self.grid.as_ref().and_then(GridConfig::validate) {
                return Some(err);
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
    /// Bid/ask per bar, asked only for a bar where a fill can happen. Absent = mid or trade
    /// bars only.
    pub quotes: Option<&'a (dyn QuoteSource + Sync)>,
    /// A lower timeframe of the same instrument, asked for a bar only when the order of events
    /// inside it matters.
    pub sub: Option<&'a (dyn SubSource + Sync)>,
}

pub use otw_store::histdata::Quote;

/// Where the engine finds a bar's bid and ask. It is asked only for a bar where a fill can
/// happen (an entry, an exit, a level within reach), so a source can load or download them on
/// demand. None = not available: that bar falls back to the mid and the spread.
pub trait QuoteSource {
    fn quote(&self, bar: usize) -> Option<Quote>;
}

impl QuoteSource for Vec<Option<Quote>> {
    fn quote(&self, bar: usize) -> Option<Quote> {
        self.get(bar).copied().flatten()
    }
}

/// Lower-timeframe candles inside one strategy bar, oldest first.
pub struct SubSlice {
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub quotes: Option<Vec<Option<Quote>>>,
}

/// Where the engine finds the candles inside a bar. It is asked only for a bar where the stop
/// and a target are both reached, or where a limit filled mid-bar, so a source can load (or
/// download) them on demand. None = not available: that bar settles by the worst case.
pub trait SubSource {
    fn candles(&self, bar: usize) -> Option<std::sync::Arc<SubSlice>>;
    /// The lower timeframe this source reads, for the run report.
    fn timeframe(&self) -> Option<String> {
        None
    }
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
    pub quotes: Option<Box<dyn QuoteSource + Send + Sync>>,
    pub sub: Option<Box<dyn SubSource + Send + Sync>>,
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
            quotes: self.quotes.as_deref().map(|q| q as &(dyn QuoteSource + Sync)),
            sub: self.sub.as_deref().map(|s| s as &(dyn SubSource + Sync)),
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
    /// How orders went: limits placed, filled and expired, bars where the order of a stop and
    /// a target mattered, and how many fills were priced off real bid/ask.
    pub execution: ExecStats,
    /// Limit orders still resting after the last bar (what a paper session has working).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending_orders: Vec<WorkingOrder>,
    /// Positions still open after the last bar, with the levels they hold for the next one:
    /// what a paper session watches on the live price.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_positions: Vec<OpenPosition>,
    /// Stretches where the market traded but no candle is stored (see [`holes`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_gaps: Vec<DataGap>,
    /// Trades a hole cut through. Their exit is unknown, so they are left out of `trades`,
    /// the cash and every statistic, and listed here for the user to see.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_trades: Vec<Trade>,
}

/// One hole in an asset's data: the last candle before it and the first one after.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct DataGap {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ticker: String,
    pub from: String,
    pub to: String,
}

/// The holes in each input, labelled for the result.
pub fn data_gaps(inputs: &[&Bars]) -> Vec<DataGap> {
    inputs
        .iter()
        .flat_map(|b| {
            holes::find(b.ts).into_iter().map(move |h| DataGap {
                ticker: b.ticker.to_string(),
                from: b.ts[h.before].clone(),
                to: b.ts[h.after].clone(),
            })
        })
        .collect()
}

/// An open position and the levels it holds once the last bar has closed (the trailing stop
/// ratcheted on it).
#[derive(Debug, Serialize, Clone)]
pub struct OpenPosition {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ticker: String,
    pub direction: String,
    pub entry_ts: String,
    pub avg_price: f64,
    pub qty: f64,
    /// Fees paid on the way in, for the P&L a live exit reports before the next run.
    pub fees: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<f64>,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_limit_reason: Option<String>,
}

/// A fill that happened on the live price between two runs of a paper session. The run that
/// follows replays it as given, on the bar it happened in, instead of re-deciding it from the
/// candle: the live price saw what the candle cannot say.
#[derive(Debug, Clone)]
pub struct ForcedFill {
    pub bar: usize,
    pub kind: ForcedKind,
}

#[derive(Debug, Clone)]
pub enum ForcedKind {
    /// The position closed at `px` (spread and slippage in) for `reason`; `maker` = a limit.
    Exit { px: f64, reason: String, maker: bool },
    /// A resting entry limit filled at `px`.
    Entry { long: bool, px: f64 },
}

/// Order and fill counters of one run. Zero everywhere on a plain market-order run, except the
/// ambiguity counts, which every run has.
#[derive(Debug, Serialize, Default, Clone)]
pub struct ExecStats {
    pub orders_placed: usize,
    pub orders_filled: usize,
    pub orders_expired: usize,
    /// Bars where the bar alone cannot tell which came first: the stop and a target both
    /// reached, or a target reached in the bar a limit entry filled.
    pub ambiguous_bars: usize,
    /// Of those, settled by walking the lower timeframe.
    pub resolved_bars: usize,
    /// Of those, settled by the worst case (the stop first, no target before a mid-bar fill).
    pub unresolved_bars: usize,
    /// Lower timeframe the run read, when it read one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_timeframe: Option<String>,
    pub fills: usize,
    /// Fills priced off stored bid/ask rather than the mid and the spread.
    pub quoted_fills: usize,
    /// Live fills (paper) the replay found no position or order for.
    pub forced_unmatched: usize,
}

/// An order working at the end of the run: a limit resting, or (forward runs) an order the
/// last close decided and the next bar sends.
#[derive(Debug, Serialize, Clone, Default)]
pub struct WorkingOrder {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ticker: String,
    /// "entry" | "add" | "exit"
    pub kind: String,
    pub direction: String,
    /// The limit price; for a market order, the last close, a provisional price until the
    /// next open fills it.
    pub price: f64,
    /// Bars of validity left after the last bar.
    pub bars_left: usize,
    /// "limit" | "market".
    pub order: String,
    /// Set on an order the last close decided: the timestamp of that bar (its signal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_ts: Option<String>,
    /// The size it is expected to fill at that price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qty: Option<f64>,
    /// The same order as an amount of money (`qty × price × multiplier`), for a broker that
    /// takes a notional order and fills it in fractional units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// ATR of the signal bar, which an ATR stop or target of the new position is sized from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr: Option<f64>,
    /// Whether the price is still to be taken off the next open (a limit anchored on it).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub provisional: bool,
    /// For such a limit: its distance from that open, a fraction of it when `anchor_pct`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_offset: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub anchor_pct: bool,
}

/// The levels a position opened at `px` holds on its first bar: its stop (the tighter of the
/// stop loss and a trail live from the entry, with the reason it names) and its take profit.
/// `atr`: the ATR of the bar before the entry, for ATR rules.
pub fn fresh_levels(s: &Settings, long: bool, px: f64, atr: Option<f64>) -> (Option<(f64, &'static str)>, Option<f64>) {
    let Some(side) = (if long { s.long.as_ref() } else { s.short.as_ref() }) else { return (None, None) };
    let own = SideSignals {
        entry: None,
        exit: None,
        sl: side.sl_rule(),
        tp: side.tp_rule(),
        trail: side.trail_rule(),
        atr: None,
        exit_on_reverse: false,
        tp_through: s.execution.take_profit_fill == "through",
        tick: s.instrument.tick_size,
    };
    let mut p = Pos::new(long, Lot { price: px, qty: 1.0, fee: 0.0 }, 0, atr, px);
    p.arm_trail(own.trail.as_ref());
    let lv = levels_for(&p, &own, 0, false);
    (lv.stop, lv.limits.iter().find(|l| l.1 == "take_profit").map(|l| l.0))
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

pub(crate) fn fee_for(f: &Fees, qty: f64, price: f64, mult: f64) -> f64 {
    let notional = qty.abs() * price.abs() * mult;
    match (f.per.as_str(), f.amount_kind.as_str()) {
        ("trade", "pct") => notional * f.amount / 100.0,
        ("trade", _) => f.amount,
        (_, "pct") => notional * f.amount / 100.0,
        (_, _) => f.amount * qty.abs(),
    }
}

/// One stacked entry inside a live position.
#[derive(Clone)]
struct Lot {
    price: f64,
    qty: f64,
    fee: f64,
}

/// Live position state during the simulation (one per asset that is currently in a trade).
#[derive(Clone)]
struct Pos {
    long: bool,
    lots: Vec<Lot>,
    /// This asset's own bar index at first entry (for entry_ts + bars_held, asset-local).
    entry_bar: usize,
    /// Asset bar index of the latest add — guards against double-adding on one bar.
    last_add_bar: usize,
    /// ATR of the last bar closed before the first entry (for ATR-based SL/TP distances; None for pct).
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
    /// A resting exit limit (exit order type "limit").
    exit_limit: Option<PendingExit>,
    /// Where the position's first bar started: past a mid-bar fill, that bar's own extremes may
    /// predate the position.
    entry_start: Start,
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
            exit_limit: None,
            entry_start: Start::Open,
        }
    }

    /// Place a trail that is live from the entry at `entry ∓ distance`, the way the order goes
    /// to the exchange with the fill, so the entry bar itself is tested against it. The closed
    /// candle then ratchets it as usual; this level is never above what that ratchet sets.
    fn arm_trail(&mut self, rule: Option<&Trail>) {
        let Some(t) = rule.filter(|t| t.value > 0.0 && t.activate_pct <= 0.0) else { return };
        let avg = self.avg_price();
        if let Some(d) = t.distance(avg) {
            self.trail_px = Some(if self.long { avg - d } else { avg + d });
        }
    }
}

/// Widen `p`'s excursions (MAE/MFE, gross, × multiplier) with prices the position lived through.
fn excursion(p: &mut Pos, prices: impl IntoIterator<Item = f64>, mult: f64) {
    for px in prices {
        let u = p.unrealized(px, mult);
        p.mae = p.mae.max((-u).max(0.0));
        p.mfe = p.mfe.max(u.max(0.0));
    }
}

/// The part of bar `bar` a position opened at `start` lived through, as its (low, high): the
/// whole bar from the open; past a mid-bar fill, the fill price, the close and the lower-timeframe
/// candles after the fill when they are known (the bar's own extremes may predate the position).
fn lived_range(b: &Bars, bar: usize, start: Start, fill_px: f64) -> (f64, f64) {
    match start {
        Start::Open => (b.low[bar], b.high[bar]),
        Start::Mid => (fill_px.min(b.close[bar]), fill_px.max(b.close[bar])),
        Start::Sub(k, _) => {
            let (mut lo, mut hi) = (fill_px.min(b.close[bar]), fill_px.max(b.close[bar]));
            if let Some(sb) = sub_slice(b, bar) {
                for j in (k + 1)..sb.high.len() {
                    lo = lo.min(sb.low[j]);
                    hi = hi.max(sb.high[j]);
                }
            }
            (lo, hi)
        }
    }
}

/// Ratchet `p`'s trailing level on closed bar `bar`: from the candle's favourable extreme, never
/// loosened, and to the average entry once the breakeven step is reached.
fn ratchet(p: &mut Pos, rule: &Trail, b: &Bars, bar: usize) {
    let (long, avg) = (p.long, p.avg_price());
    // On the bar a limit filled inside, only the part after the fill is the position's.
    let (lo, hi) = if p.entry_bar == bar {
        lived_range(b, bar, p.entry_start, p.lots.first().map_or(avg, |l| l.price))
    } else {
        (b.low[bar], b.high[bar])
    };
    let favorable = if long { hi } else { lo };
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
    if let Some(level) = level {
        p.trail_px = tighten(p.trail_px, level);
    }
}

/// The levels `p` holds through `bar`: the stop (fixed, moved by an add, or trailing, the
/// tighter wins and names the reason), the take profit and a live exit limit.
fn levels_for(p: &Pos, own: &SideSignals, bar: usize, exit_through: bool) -> Levels {
    let avg = p.avg_price();
    let sl_px = own
        .sl
        .as_ref()
        .and_then(|rule| stop_distance(rule, avg, p.atr_entry))
        .map(|d| if p.long { avg - d } else { avg + d });
    // `after_add_sl` may have moved the stop tighter than the rule stop.
    let sl_px = match (sl_px, p.stop_override) {
        (Some(rule), Some(ov)) => Some(if p.long { rule.max(ov) } else { rule.min(ov) }),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    };
    // The trailing level (ratcheted at the *previous* closed candle, never at this one)
    // competes with the fixed stop: the tighter of the two is what the market reaches
    // first, and it is the one that names the exit reason.
    let stop = match (sl_px, p.trail_px) {
        (Some(fixed), Some(tr)) => {
            if if p.long { tr > fixed } else { tr < fixed } {
                Some((tr, "trailing_stop"))
            } else {
                Some((fixed, "stop_loss"))
            }
        }
        (Some(fixed), None) => Some((fixed, "stop_loss")),
        (None, Some(tr)) => Some((tr, "trailing_stop")),
        (None, None) => None,
    };
    let mut limits = Vec::with_capacity(2);
    if let Some(tp) = own
        .tp
        .as_ref()
        .and_then(|rule| stop_distance(rule, avg, p.atr_entry))
        .map(|d| if p.long { avg + d } else { avg - d })
    {
        limits.push((tp, "take_profit", own.tp_through));
    }
    if let Some(pe) = &p.exit_limit {
        if let (true, Some(px)) = (pe.first_bar <= bar && bar <= pe.last_bar, pe.px) {
            limits.push((px, pe.reason, exit_through));
        }
    }
    // On the tick grid, against the trader: a long's stop a tick lower (a short's higher), its
    // targets and exit limits a tick further away.
    let t = own.tick;
    let stop = stop.map(|(x, r)| (on_tick(x, t, !p.long), r));
    for l in limits.iter_mut() {
        l.0 = on_tick(l.0, t, p.long);
    }
    Levels { long: p.long, stop, limits }
}

/// Whether the trading window lets an entry through on the bar after `ts`'s last one, its
/// timestamp taken one bar step on (the step between the last two bars).
fn next_bar_open(filters: &Filters, ts: &[String]) -> bool {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    let n = ts.len();
    if n < 2 {
        return true;
    }
    let (Ok(a), Ok(z)) = (OffsetDateTime::parse(&ts[n - 2], &Rfc3339), OffsetDateTime::parse(&ts[n - 1], &Rfc3339)) else {
        return true;
    };
    let Ok(next) = (z + (z - a)).format(&Rfc3339) else { return true };
    clock_gate(filters, &[next]).is_none_or(|g| g[0])
}

/// An open position as a forward run reports it once `bar` has closed: the levels it holds for
/// the next bar (`p`'s trail already ratcheted on `bar`).
fn open_position(p: &Pos, own: &SideSignals, b: &Bars, bar: usize, exit_through: bool) -> OpenPosition {
    let lv = levels_for(p, own, bar + 1, exit_through);
    let limit = lv.limits.iter().find(|l| l.1 != "take_profit");
    OpenPosition {
        ticker: b.ticker.to_string(),
        direction: if p.long { "long".into() } else { "short".into() },
        entry_ts: b.ts[p.entry_bar].clone(),
        avg_price: p.avg_price(),
        qty: p.qty(),
        fees: p.entry_fees(),
        stop: lv.stop.map(|s| s.0),
        stop_reason: lv.stop.map(|s| s.1).unwrap_or("stop_loss").to_string(),
        take_profit: lv.limits.iter().find(|l| l.1 == "take_profit").map(|l| l.0),
        exit_limit: limit.map(|l| l.0),
        exit_limit_reason: limit.map(|l| l.1.to_string()),
    }
}

/// The price of a limit `spec` places on `bar` for a signal that closed on `signal_bar`: below
/// the reference to buy, above it to sell. None when an ATR offset has no value yet.
fn limit_price(spec: &OrderSpec, b: &Bars, signal_bar: usize, bar: usize, buy: bool, atr: Option<&Vec<Option<f64>>>) -> Option<f64> {
    let reference = if spec.reference == "open" { b.open[bar] } else { b.close[signal_bar] };
    let d = spec.distance(reference, atr.and_then(|a| a[signal_bar]))?;
    Some(if buy { reference - d } else { reference + d })
}

/// The exit limit a signal decided at the close of `bar - 1` places: resting from `bar` for
/// `valid` bars, above the price for a long (below for a short). Priced off that close, or off
/// `bar`'s open, which a forward run does not know yet for the bar after its last: the order
/// then carries its distance and takes its price on that open. None when an ATR offset has no
/// value yet.
fn exit_limit_order(spec: &OrderSpec, b: &Bars, bar: usize, long: bool, atr: Option<&Vec<Option<f64>>>, valid: usize, reason: &'static str) -> Option<PendingExit> {
    let signal = bar.checked_sub(1)?;
    let last_bar = bar + valid.max(1) - 1;
    if bar < b.open.len() || spec.reference != "open" {
        let px = limit_price(spec, b, signal, bar.min(b.open.len() - 1), !long, atr)?;
        return Some(PendingExit { px: Some(px), offset: 0.0, pct: 0.0, first_bar: bar, last_bar, reason });
    }
    let (offset, pct) = match spec.offset_kind.as_str() {
        "pct" => (0.0, spec.offset),
        _ => (spec.distance(b.close[signal], atr.and_then(|a| a[signal]))?, 0.0),
    };
    Some(PendingExit { px: None, offset, pct, first_bar: bar, last_bar, reason })
}

/// What the close of `bar` (a forward run's last) decided for position `p` at the next open.
/// A signal exit sent as a limit is placed on `p`, resting from that open; returns whether the
/// position leaves at that open at market (a market signal exit, or an exit limit that expired
/// unfilled at this close and falls back to market).
fn plan_next_open(p: &mut Pos, own: &SideSignals, b: &Bars, bar: usize, spec: &OrderSpec, atr: Option<&Vec<Option<f64>>>, valid: usize) -> bool {
    let decided = |sig: &Option<Vec<bool>>| sig.as_ref().is_some_and(|v| v[bar]);
    let reason = if decided(&own.exit) {
        Some("exit_signal")
    } else if own.exit_on_reverse && !decided(&own.entry) {
        Some("signal")
    } else {
        None
    };
    let expired = p.exit_limit.as_ref().is_some_and(|pe| pe.last_bar <= bar);
    if spec.is_limit() {
        if expired {
            p.exit_limit = None;
        }
        if p.exit_limit.is_none() {
            if let Some(r) = reason {
                p.exit_limit = exit_limit_order(spec, b, bar + 1, p.long, atr, valid, r);
            }
        }
        return expired && spec.on_expiry != "cancel";
    }
    reason.is_some()
}

/// The entry bar of a position that just filled: excursions over the part of the bar it lived,
/// then its stop and target against that same bar. Returns the position when it exits there.
#[allow(clippy::too_many_arguments)]
fn entry_bar_exit(a: &mut Asset, bar: usize, start: Start, fill: &FillModel, intrabar: bool, exit_through: bool, mult: f64, st: &mut ExecStats) -> Option<(Pos, ExitFill)> {
    let b = a.b;
    let p = a.pos.as_mut()?;
    p.entry_start = start;
    let entry_px = p.avg_price();
    let e = match a.forced_exit.remove(&bar) {
        Some((px, reason, maker)) => Some(ExitFill { px, reason, maker, quoted: false }),
        None => {
            let own = if p.long { &a.long } else { &a.short };
            let lv = levels_for(p, own, bar, exit_through);
            fill.bar_exit(b, bar, &lv, start, intrabar, st)
        }
    };
    let p = a.pos.as_mut()?;
    match e {
        // Out inside its first bar: it lived from its fill to its exit, nothing after.
        Some(e) => {
            excursion(p, [entry_px, e.px], mult);
            Some((a.pos.take()?, e))
        }
        None => {
            let (lo, hi) = lived_range(b, bar, start, entry_px);
            excursion(p, [lo, hi], mult);
            None
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
    /// The take profit fills only once the price trades beyond it.
    tp_through: bool,
    /// Instrument tick the levels are placed on (0 = none).
    tick: f64,
}

fn side_signals(side: Option<&Side>, enabled: bool, b: &Bars, defs: &IndicatorDefs, ex: &Execution, tick: f64) -> SideSignals {
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
        tp_through: ex.take_profit_fill == "through",
        tick,
    }
}

/// Resolve a stop rule to an **absolute price distance** from the average entry. For `pct` the
/// distance scales with `avg` (so it re-anchors when pyramiding moves the average); for `atr` it
/// is the ATR of the last bar closed before the first entry times the multiple (fixed distance).
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

/// Whether what asset `a` did at its open on this row is known at the instant `at` an order is
/// sent: its bar opened at or before `at`. A row is a period, and two sessions share it (a crypto
/// day opens at 00:00 UTC, a New York one at 13:30): the later one is still at its previous close
/// for an order sent at the earlier open. Without a clock (`at` None, unparseable stamps) the
/// whole row counts as known.
fn seen(a: &Asset, at: Option<i64>) -> bool {
    match (at, a.t_open) {
        (None, _) => true,
        (Some(at), Some(t)) => t <= at,
        (Some(_), None) => false,
    }
}

/// Mark-to-market portfolio equity for an order sent at `at`: `cash` (as the row opened) plus
/// every position held as the row opened, at its asset's open when that open is known by then,
/// else at its previous close. Entries happen at an open, so this is the "equity" sizing rules,
/// limits and breakers evaluate against: the close of the same bar is not known yet.
fn mtm_equity(cash: f64, assets: &[Asset], at: Option<i64>) -> f64 {
    cash + assets.iter().map(|a| if seen(a, at) { a.open_unreal } else { a.pre_unreal }).sum::<f64>()
}

/// Notional an asset holds for an order sent at `at`. A position closed later inside its bar
/// still held it then, so it keeps counting; an asset whose bar opens later still holds what
/// it held at its previous close.
fn open_notional(a: &Asset, mult: f64, at: Option<i64>) -> f64 {
    if !seen(a, at) {
        return a.pre_notional;
    }
    a.pos.as_ref().map(|p| p.notional(a.mark, mult)).unwrap_or(a.held_open)
}

/// Whether an asset holds a position for an order sent at `at` (same rule as `open_notional`).
fn held_at(a: &Asset, at: Option<i64>) -> bool {
    if !seen(a, at) {
        return a.pre_held;
    }
    a.pos.is_some() || a.held_open > 0.0
}

/// Margin already committed by open positions (open notional / leverage), so a new entry's
/// margin check runs against the *free* portion of equity rather than the whole of it.
fn committed_margin(assets: &[Asset], mult: f64, lev: f64, at: Option<i64>) -> f64 {
    assets.iter().map(|a| open_notional(a, mult, at)).sum::<f64>() / lev.max(1e-9)
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
    /// Price known at the current row's open: the open where the asset has a bar, else the
    /// last close. What entries at that open are sized and gated on.
    mark: f64,
    /// Notional held at the current row's open by a position since closed inside the bar.
    held_open: f64,
    /// This asset's bar times, epoch seconds (None where a stamp does not parse).
    epochs: Vec<Option<i64>>,
    /// When this row's bar opens (None where the asset has no bar on the row).
    t_open: Option<i64>,
    /// The position held as the row opened: unrealized at the open, and unrealized, notional and
    /// presence at the previous close (what an earlier session's order sees of it).
    open_unreal: f64,
    pre_unreal: f64,
    pre_notional: f64,
    pre_held: bool,
    /// A resting entry (or pyramid add) limit order.
    pending: Option<PendingEntry>,
    /// ATR series behind an ATR-offset entry / exit limit, when one is configured.
    entry_atr: Option<Vec<Option<f64>>>,
    exit_atr: Option<Vec<Option<f64>>>,
    /// Bar on which a position closed after the open (a stop, a target, a limit, a close).
    /// No entry fills on that bar: its open came before the exit.
    exited_after_open: Option<usize>,
    /// Fills a paper session took on the live price, by bar.
    forced_exit: HashMap<usize, (f64, &'static str, bool)>,
    forced_entry: HashMap<usize, (bool, f64)>,
    /// Per bar: the last one before a hole in this asset's data.
    gap_after: Vec<bool>,
    /// Per bar: no entry fills here, the strategy's inputs straddle a hole.
    cold: Vec<bool>,
}

/// A resting entry limit: buy below (long) or sell above (short) `px`, live on this asset's bars
/// up to `last_bar`. `add` = a pyramid add to the open position.
struct PendingEntry {
    long: bool,
    add: bool,
    px: f64,
    last_bar: usize,
}

/// A resting exit limit on an open position. `px` is None until its first bar when it is
/// anchored on that bar's open (`offset` is then applied to it).
#[derive(Clone)]
struct PendingExit {
    px: Option<f64>,
    /// Distance from the anchor: a price, plus a fraction of the anchor (pct anchored on an open).
    offset: f64,
    pct: f64,
    first_bar: usize,
    last_bar: usize,
    reason: &'static str,
}

/// A closed-trade summary the sizing layer reads (Kelly window).
#[derive(Clone)]
struct ClosedTrade {
    /// Per-trade return on notional, as a fraction (net_pnl / entry notional).
    ret: f64,
    /// Net profit above zero. A breakeven trade is neither a win nor a loss, as in the stats.
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
                let ls: Vec<f64> = recent.iter().filter(|t| !t.win && t.ret < 0.0).map(|t| -t.ret).collect();
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

/// A sizing whose numbers cannot size anything: a size, a percent or a risk of zero or below.
fn sizing_error(sizing: &Sizing) -> Option<String> {
    let pos = |x: f64| x.is_finite() && x > 0.0;
    match sizing {
        Sizing::FixedQty { qty } if !pos(*qty) => Some("the fixed quantity must be above 0".into()),
        Sizing::PercentEquity { percent } if !pos(*percent) => Some("the percent of equity must be above 0".into()),
        Sizing::Risk { risk_pct } if !pos(*risk_pct) => Some("the risk per trade must be above 0".into()),
        Sizing::EquityTiers { tiers, .. } if tiers.iter().any(|t| !t.above.is_finite() || !(t.value.is_finite() && t.value >= 0.0)) => {
            Some("equity tiers need a threshold and a value of 0 or more".into())
        }
        Sizing::Kelly { fraction, window, cap_pct, warmup } => {
            if !pos(*fraction) || *window == 0 || !pos(*cap_pct) {
                return Some("Kelly sizing needs a fraction, a window and a cap above 0".into());
            }
            warmup.as_deref().and_then(sizing_error)
        }
        _ => None,
    }
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
        Operand::Indicator { indicator, period, fast, slow, mult, signal_period } => {
            builtin_lookback(indicator, *period, *fast, *slow, *mult, *signal_period)
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

/// Bars a built-in indicator needs before its first value, plus the one a crossing compares
/// with. Measured on the indicator itself over a synthetic series, so a length left at its
/// default and a chained length (MACD's signal over its slow average) count as computed.
fn builtin_lookback(indicator: &str, period: usize, fast: usize, slow: usize, mult: f64, signal_period: usize) -> usize {
    let configured = [period, fast, slow, signal_period].into_iter().max().unwrap_or(0);
    let n = configured * 4 + 1000;
    let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.7).sin() * 5.0 + (i as f64 * 0.13).cos() * 3.0).collect();
    let open: Vec<f64> = (0..n).map(|i| if i == 0 { close[0] } else { close[i - 1] }).collect();
    let high: Vec<f64> = (0..n).map(|i| open[i].max(close[i]) + 1.0).collect();
    let low: Vec<f64> = (0..n).map(|i| open[i].min(close[i]) - 1.0).collect();
    let volume: Vec<f64> = (0..n).map(|i| 1000.0 + (i % 7) as f64 * 100.0).collect();
    let ts: Vec<String> = (0..n as i64)
        .map(|i| {
            let t = time::OffsetDateTime::from_unix_timestamp(1_704_067_200 + i * 3600).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
        })
        .collect();
    let b = Bars { ticker: "", ts: &ts, open: &open, high: &high, low: &low, close: &close, volume: &volume, quotes: None, sub: None };
    match resolve_builtin(indicator, period, fast, slow, mult, signal_period, &b).iter().position(Option::is_some) {
        Some(first) => first + 1,
        None => configured,
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

/// Rows of the clock a run over `assets` walks, and the rows per year that clock measures: the
/// same figure `risk_ratios` annualizes the Sharpe with, so a test that de-annualizes it (the
/// deflated Sharpe) undoes exactly what was done. None when the stamps carry no time span.
pub fn clock_rate(assets: &[&Bars]) -> (usize, Option<f64>) {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    let (clock, _) = merged_clock(assets);
    let n = clock.len();
    let span = match (clock.first(), clock.last()) {
        (Some(a), Some(z)) => match (OffsetDateTime::parse(a, &Rfc3339), OffsetDateTime::parse(z, &Rfc3339)) {
            (Ok(a), Ok(z)) => (z - a).whole_seconds() as f64,
            _ => 0.0,
        },
        _ => 0.0,
    };
    let rate = (n > 1 && span > 0.0).then(|| (n - 1) as f64 / (span / (365.25 * 24.0 * 3600.0)));
    (n, rate)
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
/// limits against the state known at their open. Exit priority per bar: explicit exit group (at
/// open) → window end (at open) → stop-and-reverse (at open) → entry-group reversal (at open) →
/// expired exit limit (at open) → a pyramid add (at the open, or a resting limit inside the bar)
/// → SL / TP / exit limit (a gap past a level fills at the open; inside the bar the stop wins a
/// tie unless the lower timeframe says otherwise) → the asset's last bar (at close). A position
/// is tested against its stop and target on the bar it opens in.
pub fn run_portfolio(s: &Settings, inputs: &[&Bars]) -> RunResult {
    run_engine(s, inputs, &[], false)
}

/// The exit reason a live fill names, as the engine spells it.
fn reason_of(r: &str) -> &'static str {
    match r {
        "stop_loss" => "stop_loss",
        "trailing_stop" => "trailing_stop",
        "take_profit" => "take_profit",
        "exit_signal" => "exit_signal",
        "signal" => "signal",
        _ => "live",
    }
}

/// [`run_portfolio`] run forward, the way a paper session runs it: every bar has closed in
/// reality, the last one included, so a position opened at the last bar's open is held into the
/// next one rather than skipped, and what the last close decided (an exit limit, its expiry, a
/// reverse) happens. `forced`: the fills the session took on the live price, per asset (in
/// input order). They replace what the candle would decide on their bar; one the replay has no
/// position or order for is counted in `execution.forced_unmatched`.
pub fn run_portfolio_forced(s: &Settings, inputs: &[&Bars], forced: &[Vec<ForcedFill>]) -> RunResult {
    run_engine(s, inputs, forced, true)
}

/// The signal engine. `forward`: the last bar is the bar that just closed, not the end of
/// history (see [`run_portfolio_forced`]).
fn run_engine(s: &Settings, inputs: &[&Bars], forced: &[Vec<ForcedFill>], forward: bool) -> RunResult {
    // Grid and DCA are separate order generators, dispatched here (grid is single-asset:
    // it ladders one instrument, so it reads the first dataset).
    if s.kind == "grid" {
        return run_grid(s, inputs[0]);
    }
    if s.kind == "dca" {
        return dca::run_dca(s, inputs);
    }
    let pyramiding = s.pyramiding.clamp(1, 20);
    let allow_long = s.allow_long();
    let allow_short = s.allow_short();
    let mult = if s.instrument.multiplier > 0.0 { s.instrument.multiplier } else { 1.0 };
    // Fill mechanics bundled once (spread or bid/ask, slippage), threaded to every fill.
    let fill = FillModel::new(s);
    let ex = &s.execution;
    let intrabar = ex.intrabar;
    let entry_limit = ex.entry_order.is_limit();
    let exit_limit = ex.exit_order.is_limit();
    let entry_through = ex.entry_order.fill == "through";
    let exit_through = ex.exit_order.fill == "through";
    let entry_valid = ex.entry_order.valid_bars.max(1);
    let exit_valid = ex.exit_order.valid_bars.max(1);
    let exit_expiry_market = ex.exit_order.on_expiry != "cancel";
    // Market fills pay the fee as configured; resting limits pay the maker fee when one is set.
    let fees_taker = s.fees.clone();
    let fees_maker = Fees { amount: ex.maker_fee.unwrap_or(s.fees.amount), ..s.fees.clone() };
    let fees_of = |maker: bool| if maker { &fees_maker } else { &fees_taker };
    let mut st = ExecStats {
        sub_timeframe: inputs.iter().find_map(|b| b.sub.and_then(|sb| sb.timeframe())).filter(|_| intrabar),
        ..ExecStats::default()
    };

    let (clock, maps) = merged_clock(inputs);
    let m = clock.len();
    // Kelly reads the strategy's own track record: every trade its signals produce, including
    // the ones it skipped for a non-positive edge (the theoretical equity curve). Fed only by the
    // trades it took, a run whose first window went negative skipped every later entry and never
    // saw the edge come back. The record is a run of the same strategy at the Kelly cap.
    let shadow: Option<(Vec<String>, Vec<ClosedTrade>)> = match &s.sizing {
        Sizing::Kelly { cap_pct, .. } => {
            let mut theory = s.clone();
            theory.sizing = Sizing::PercentEquity { percent: cap_pct.clamp(0.01, 100.0) };
            let r = run_engine(&theory, inputs, forced, forward);
            let mut t: Vec<(String, ClosedTrade)> = r
                .trades
                .iter()
                .map(|t| (t.exit_ts.clone(), ClosedTrade { ret: t.return_pct / 100.0, win: t.pnl > 0.0 }))
                .collect();
            t.sort_by(|a, b| a.0.cmp(&b.0));
            Some(t.into_iter().unzip())
        }
        _ => None,
    };

    let order_atr = |spec: &OrderSpec, b: &Bars| spec.atr_period().map(|p| indicators::atr(b.high, b.low, b.close, p));
    // Holes in each asset's data, and the bars it takes every input to be whole again after one.
    let asset_holes: Vec<Vec<holes::Hole>> = inputs.iter().map(|b| holes::find(b.ts)).collect();
    let hole_warmup = hole_warmup(s);
    let mut assets: Vec<Asset> = inputs
        .iter()
        .zip(maps.into_iter())
        .enumerate()
        .map(|(ai, (b, bar_at_row))| Asset {
            b,
            long: side_signals(s.long.as_ref(), allow_long, b, &s.indicators, &s.execution, s.instrument.tick_size),
            short: side_signals(s.short.as_ref(), allow_short, b, &s.indicators, &s.execution, s.instrument.tick_size),
            bar_at_row,
            pos: None,
            active_pos_rows: 0,
            last_close: b.close.first().copied().unwrap_or(0.0),
            mark: b.open.first().copied().unwrap_or(0.0),
            held_open: 0.0,
            epochs: Vec::new(),
            t_open: None,
            open_unreal: 0.0,
            pre_unreal: 0.0,
            pre_notional: 0.0,
            pre_held: false,
            pending: None,
            entry_atr: order_atr(&ex.entry_order, b),
            exit_atr: order_atr(&ex.exit_order, b),
            exited_after_open: None,
            forced_exit: HashMap::new(),
            forced_entry: HashMap::new(),
            gap_after: holes::before_mask(b.ts.len(), &asset_holes[ai]),
            cold: holes::cold_mask(b.ts.len(), &asset_holes[ai], hole_warmup),
        })
        .collect();

    // Bar times, to order a row's opens when its assets keep different sessions. Only a
    // multi-asset run needs them.
    if inputs.len() > 1 {
        use time::{format_description::well_known::Rfc3339, OffsetDateTime};
        for a in assets.iter_mut() {
            a.epochs = a.b.ts.iter().map(|t| OffsetDateTime::parse(t, &Rfc3339).ok().map(|d| d.unix_timestamp())).collect();
        }
    }
    for (a, fills) in assets.iter_mut().zip(forced) {
        for f in fills {
            match &f.kind {
                ForcedKind::Exit { px, reason, maker } => {
                    a.forced_exit.insert(f.bar, (*px, reason_of(reason), *maker));
                }
                ForcedKind::Entry { long, px } => {
                    a.forced_entry.insert(f.bar, (*long, *px));
                }
            }
        }
    }

    let mut trades: Vec<Trade> = Vec::new();
    // Trades a hole cut through: kept apart, never booked.
    let mut excluded: Vec<Trade> = Vec::new();
    let mut working: Vec<WorkingOrder> = Vec::new();
    let mut open_now: Vec<OpenPosition> = Vec::new();
    // Exit costs booked on the positions an "end" closes only on paper.
    let mut end_cost = 0.0f64;
    // Tickers whose open position a forward run sees leave at the next open.
    let mut leaving_at_open: std::collections::HashSet<String> = std::collections::HashSet::new();
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

    // Settle one exit: the trade, the cash, the Kelly history and the fill counters. A position
    // closed by "end" is still open in reality, so a resting exit limit it holds is reported.
    macro_rules! settle {
        ($b:expr, $p:expr, $e:expr, $bar:expr) => {{
            let (b, p, e, bar): (&Bars, &Pos, ExitFill, usize) = ($b, $p, $e, $bar);
            if e.reason == "data_gap" {
                // Nothing is known about how it ended: the trade is listed, not booked. The cash
                // is as if it never happened, and the Kelly history never sees it.
                let mut void_cash = 0.0;
                close_trade(&mut excluded, &mut void_cash, fees_of(e.maker), mult, b, p, e.px, e.reason, bar);
                // Its entry fees left the cash at the fill: they come back with the trade.
                cash += p.entry_fees();
            } else {
            if e.reason == "end" {
                // Still open in reality: what closing it on paper cost (the spread to the close
                // and the exit fee) is not money the account has lost.
                end_cost += p.unrealized(b.close[bar], mult) - p.unrealized(e.px, mult)
                    + fee_for(fees_of(e.maker), p.qty(), e.px, mult);
            }
            let ct = close_trade(&mut trades, &mut cash, fees_of(e.maker), mult, b, p, e.px, e.reason, bar);
            closed.push(ct);
            st.fills += 1;
            st.quoted_fills += e.quoted as usize;
            if e.maker && e.reason != "take_profit" {
                st.orders_filled += 1;
            } else if p.exit_limit.is_some() {
                // Closed by something else first: the resting exit is cancelled with it.
                st.orders_expired += 1;
            }
            if e.reason == "end" {
                if let Some(pe) = p.exit_limit.as_ref().filter(|pe| pe.last_bar > bar) {
                    st.orders_expired -= 1;
                    working.push(WorkingOrder {
                        ticker: b.ticker.to_string(),
                        kind: "exit".into(),
                        direction: if p.long { "long".into() } else { "short".into() },
                        price: pe.px.unwrap_or(f64::NAN),
                        bars_left: pe.last_bar - bar,
                        order: "limit".into(),
                        ..WorkingOrder::default()
                    });
                }
            }
            }
        }};
    }

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

        // What is known at this row's open: the equity and the exposure entries are decided on.
        for a in assets.iter_mut() {
            a.mark = a.bar_at_row[r].map_or(a.last_close, |bar| a.b.open[bar]);
            a.held_open = a.pos.as_ref().map_or(0.0, |p| p.notional(a.mark, mult));
            a.t_open = a.bar_at_row[r].and_then(|bar| a.epochs.get(bar).copied().flatten());
            a.open_unreal = a.pos.as_ref().map_or(0.0, |p| p.unrealized(a.mark, mult));
            a.pre_unreal = a.pos.as_ref().map_or(0.0, |p| p.unrealized(a.last_close, mult));
            a.pre_notional = a.pos.as_ref().map_or(0.0, |p| p.notional(a.last_close, mult));
            a.pre_held = a.pos.is_some();
        }
        // The cash as the row opened: exits inside it are not known to an order sent at an open.
        let cash_open = cash;
        // Assets in the order their bars open on this row (submission order on a tie, or with
        // no clock): an order sent at an earlier session's open is decided before a later one's.
        let mut order: Vec<usize> = (0..assets.len()).collect();
        order.sort_by_key(|&ai| assets[ai].t_open.unwrap_or(i64::MIN));
        // The closed trades known at this open: those before the row, and those closed at an
        // open at or before it (a market exit there). One closed later inside a bar is not.
        let closed_at_open = closed.len();
        let mut open_closes: Vec<(Option<i64>, ClosedTrade)> = Vec::new();
        // Kelly's theoretical record: the trades that closed on an earlier row.
        let shadow_known: Option<&[ClosedTrade]> =
            shadow.as_ref().map(|(ts, ct)| &ct[..ts.partition_point(|t| t.as_str() < clock[r].as_str())]);
        macro_rules! known_closed {
            ($tmp:ident, $at:expr) => {{
                let at: Option<i64> = $at;
                if let Some(sk) = shadow_known {
                    sk
                } else {
                let known_here = |t: &Option<i64>| match (at, t) {
                    (Some(a), Some(t)) => *t <= a,
                    _ => true,
                };
                if !open_closes.iter().any(|(t, _)| known_here(t)) {
                    &closed[..closed_at_open]
                } else {
                    $tmp = closed[..closed_at_open]
                        .iter()
                        .cloned()
                        .chain(open_closes.iter().filter(|(t, _)| known_here(t)).map(|(_, c)| c.clone()))
                        .collect::<Vec<_>>();
                    &$tmp[..]
                }
                }
            }};
        }

        // ── Circuit breakers: are NEW entries and adds halted this row? (positions still exit) ──
        // At the open: exits later inside the bar are not known when its orders are decided.
        // Per order: each asset's open sees the equity known then.
        let halted_at = |assets: &[Asset], at: Option<i64>| breaker_halted(&s.risk, mtm_equity(cash_open, assets, at), peak, day_start_equity);
        if halted_at(&assets, None) {
            halted_bars += 1;
        }

        // ── (1) Manage every open position at this row ──
        for &ai in &order {
            let Some(bar) = assets[ai].bar_at_row[r] else { continue };
            let b = assets[ai].b;
            assets[ai].last_close = b.close[bar];
            let is_last_bar = bar == b.ts.len() - 1;

            let Some(p) = assets[ai].pos.as_mut() else { continue };
            let p_long = p.long;
            // An exit limit anchored on the open takes its price on its first bar.
            if let Some(pe) = p.exit_limit.as_mut() {
                if pe.px.is_none() && pe.first_bar <= bar {
                    let o = b.open[pe.first_bar];
                    let d = pe.offset + o.abs() * pe.pct;
                    pe.px = Some(if p_long { o + d } else { o - d });
                }
            }
            let own = if p_long { &assets[ai].long } else { &assets[ai].short };
            let own_exit = &own.exit;
            let own_reverse = own.exit_on_reverse;
            // ATR for a potential stop-and-reverse entry comes from the *opposite* side.
            // The last closed bar's ATR, as for any entry at this open.
            let reverse_atr = if p_long { &assets[ai].short } else { &assets[ai].long }
                .atr
                .as_ref()
                .and_then(|v| bar.checked_sub(1).and_then(|k| v[k]));
            // `exit_on_reverse`: the entry group stopped holding at the last close. Decided there,
            // so it leaves at this open like any other signal exit.
            let own_reverse_fires = own_reverse && bar > 0 && !holds(&own.entry, bar - 1);
            let opp_fires = if p_long { fired(&assets[ai].short.entry, bar) } else { fired(&assets[ai].long.entry, bar) };
            // Bid/ask is read only when a market exit actually fills on this bar.
            let at_open = |reason| {
                let c = fill.qcandle(b, bar);
                ExitFill { px: fill.market_exit(c.o, c.q, false, p_long), reason, maker: false, quoted: fill.quote(&c).is_some() }
            };
            let at_close = |reason| {
                let c = fill.qcandle(b, bar);
                ExitFill { px: fill.market_exit(b.close[bar], c.q, true, p_long), reason, maker: false, quoted: fill.quote(&c).is_some() }
            };

            let mut exit: Option<(ExitFill, bool)> = None; // (fill, reverse?)
            // Set by a market exit at this bar's open, the one exit that leaves the open free.
            let mut left_at_open = false;
            // A signal exit decided at the last close: market at this open, or a limit resting
            // from it.
            let mut signal_exit = |reason: &'static str, exit: &mut Option<(ExitFill, bool)>, left: &mut bool, a: &mut Asset| {
                if !exit_limit {
                    *exit = Some((at_open(reason), false));
                    *left = true;
                } else if a.pos.as_ref().is_some_and(|p| p.exit_limit.is_none()) {
                    if let Some(pe) = exit_limit_order(&ex.exit_order, b, bar, p_long, a.exit_atr.as_ref(), exit_valid, reason) {
                        a.pos.as_mut().unwrap().exit_limit = Some(pe);
                        st.orders_placed += 1;
                    }
                }
            };
            if fired(own_exit, bar) {
                signal_exit("exit_signal", &mut exit, &mut left_at_open, &mut assets[ai]);
            }
            // Window closed and the user asked to be flat outside it: out at this bar's open,
            // the first price available once the session is over.
            if exit.is_none() && !window_open && flat_on_close {
                exit = Some((at_open("session_end"), false));
                left_at_open = true;
            }
            // A stop-and-reverse *opens* a position, so it obeys the window like any entry.
            if exit.is_none() && window_open && s.stop_and_reverse && opp_fires {
                exit = Some((at_open("reverse"), true));
                left_at_open = true;
            }
            if exit.is_none() && own_reverse_fires {
                signal_exit("signal", &mut exit, &mut left_at_open, &mut assets[ai]);
            }
            // An exit limit that rested through its last bar unfilled expired at that close: the
            // market order it falls back to goes at this open.
            if exit.is_none() {
                let p = assets[ai].pos.as_mut().unwrap();
                if p.exit_limit.as_ref().is_some_and(|pe| pe.last_bar < bar) {
                    let pe = p.exit_limit.take().unwrap();
                    st.orders_expired += 1;
                    if exit_expiry_market {
                        exit = Some((at_open(pe.reason), false));
                        left_at_open = true;
                    }
                }
            }
            // ── Pyramid add, sent at this open (market) or resting inside the bar (limit) ──
            // Decided before the bar's stop and target are tested: the add fills first, then the
            // bar is tested on the position it made (its new average, a stop it moved).
            let mut start = Start::Open;
            let at = assets[ai].t_open;
            if exit.is_none() && !halted_at(&assets, at) && (window_open || !block_adds) {
                let p = assets[ai].pos.as_ref().unwrap();
                let own = if p_long { &assets[ai].long } else { &assets[ai].short };
                let lv_old = levels_for(p, own, bar, exit_through);
                // A level the bar opened beyond fills at that open, before any add.
                let gapped = !matches!(fill.scan(&open_only(fill.qcandle_near(b, bar, lv_old.stop.map(|x| x.0).into_iter().chain(lv_old.limits.iter().map(|l| l.0)))), &lv_old, true, false), Scan::Quiet);
                let own_fires = fired(&own.entry, bar);
                let room = p.lots.len() < pyramiding && p.last_add_bar != bar;
                let stale = assets[ai].pending.as_ref().is_some_and(|pd| pd.add && pd.last_bar < bar);
                if stale {
                    assets[ai].pending = None;
                    st.orders_expired += 1;
                }
                let fill_allowed = !is_last_bar || forward;
                let mut add: Option<(f64, bool, bool)> = None; // (price, maker, quoted)
                if !gapped && room {
                    if !entry_limit {
                        if own_fires && fill_allowed {
                            let c = fill.qcandle(b, bar);
                            let px = fill.market_entry(&c, p_long);
                            if pyramid_distance_ok(&s.pyramid_steps, assets[ai].pos.as_ref().unwrap(), px) {
                                add = Some((px, false, fill.quote(&c).is_some()));
                            }
                        }
                    } else {
                        if own_fires && assets[ai].pending.is_none() {
                            if let Some(px) = limit_price(&ex.entry_order, b, bar - 1, bar, p_long, assets[ai].entry_atr.as_ref()) {
                                assets[ai].pending = Some(PendingEntry { long: p_long, add: true, px, last_bar: bar + entry_valid - 1 });
                                st.orders_placed += 1;
                            }
                        }
                        let resting = assets[ai].pending.as_ref().filter(|pd| pd.add).map(|pd| pd.px);
                        if let (Some(px), true) = (resting, fill_allowed) {
                            if let Some((fpx, fstart, fq)) = fill.entry_limit_fill(b, bar, px, p_long, entry_through, intrabar) {
                                let p = assets[ai].pos.as_ref().unwrap();
                                // The old stop sits at or beyond the add's price: the market reaches
                                // it first on the way down (up for a short), and the position is gone.
                                let stop_first = lv_old.stop.is_some_and(|(sp, _)| if p_long { sp >= fpx } else { sp <= fpx });
                                // On the lower timeframe, the old levels may exit in a candle before the add's.
                                let before = match fstart {
                                    Start::Sub(k, _) => fill.exit_before_sub(b, bar, &lv_old, k),
                                    _ => None,
                                };
                                if let Some(e) = before {
                                    exit = Some((e, false));
                                } else if !stop_first && pyramid_distance_ok(&s.pyramid_steps, p, fpx) {
                                    add = Some((fpx, true, fq));
                                    start = fstart;
                                }
                            }
                        }
                    }
                }
                let equity_now = mtm_equity(cash_open, &assets, at);
                if let Some((px, maker, fquoted)) = add.filter(|a| limits_allow_open(s, &assets, ai, a.0, mult, equity_now, true, at)) {
                    let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
                    let margin_free = equity_now - committed_margin(&assets, mult, lev, at);
                    let known;
                    let p = assets[ai].pos.as_ref().unwrap();
                    let n_lots = p.lots.len();
                    let stop_px = first_stop_px(s, p_long, p.atr_entry, px);
                    let ctx = SizeCtx { equity: equity_now, entry_px: px, stop_px, leverage: s.leverage, mult, closed: known_closed!(known, at) };
                    let scale_k = step_scale(&s.pyramid_steps.scale, n_lots);
                    match make_lot_checked(&s.sizing, fees_of(maker), &s.instrument, mult, px, scale_k, margin_free, &ctx) {
                        LotOutcome::Ok(lot) => {
                            if exposure_allows_lot(s, &assets, ai, &lot, mult, equity_now, at) {
                                if maker {
                                    assets[ai].pending = None;
                                }
                                let p = assets[ai].pos.as_mut().unwrap();
                                let q = p.qty() + lot.qty;
                                let new_avg = (p.lots.iter().map(|l| l.price * l.qty).sum::<f64>() + lot.price * lot.qty) / q;
                                cash -= lot.fee;
                                p.lots.push(lot);
                                p.last_add_bar = bar;
                                apply_after_add_sl(&s.pyramid_steps, p, new_avg);
                                st.orders_filled += maker as usize;
                                st.fills += 1;
                                st.quoted_fills += fquoted as usize;
                                // An add inside the bar can move the stop to the wrong side of the
                                // market (breakeven on an average above an add made lower down): it
                                // triggers there and then, at the price the market stood at, never
                                // at a stop price the bar may not have traded since.
                                if !matches!(start, Start::Open) {
                                    let p = assets[ai].pos.as_ref().unwrap();
                                    let own = if p_long { &assets[ai].long } else { &assets[ai].short };
                                    let beyond = |sp: f64| if p_long { sp >= px } else { sp <= px };
                                    if let Some((_, reason)) = levels_for(p, own, bar, exit_through).stop.filter(|(sp, _)| beyond(*sp)) {
                                        exit = Some((ExitFill { px: fill.exit(px, p_long), reason, maker: false, quoted: false }, false));
                                    }
                                }
                            } else {
                                skipped_exposure += 1;
                                start = Start::Open;
                            }
                        }
                        other => {
                            start = Start::Open;
                            match other {
                                LotOutcome::BelowMin => skipped_min_size += 1,
                                LotOutcome::Margin => skipped_margin += 1,
                                _ => {}
                            }
                        }
                    }
                } else {
                    start = Start::Open;
                }
            }
            // A fill the live price took: it replaces what the candle would decide.
            if exit.is_none() {
                if let Some((px, reason, maker)) = assets[ai].forced_exit.remove(&bar) {
                    exit = Some((ExitFill { px, reason, maker, quoted: false }, false));
                }
            }
            // Stop, take profit and a resting exit limit, in the order the market reaches them.
            if exit.is_none() {
                let p = assets[ai].pos.as_ref().unwrap();
                let own = if p_long { &assets[ai].long } else { &assets[ai].short };
                let lv = levels_for(p, own, bar, exit_through);
                exit = fill.bar_exit(b, bar, &lv, start, intrabar, &mut st).map(|e| (e, false));
            }
            // The asset's own last bar closes any still-open position (mirrors single-asset "end").
            if exit.is_none() && is_last_bar {
                exit = Some((at_close("end"), false));
            }

            // Excursions over the part of the bar the position lived: the whole bar when it is
            // still held at the close, from the open to the exit when it left inside the bar.
            {
                let p = assets[ai].pos.as_mut().unwrap();
                match &exit {
                    Some((e, _)) if e.reason != "end" => {
                        let first = if left_at_open { e.px } else { b.open[bar] };
                        excursion(p, [first, e.px], mult);
                    }
                    _ => {
                        excursion(p, [b.low[bar], b.high[bar]], mult);
                        let favorable = if p_long { b.high[bar] } else { b.low[bar] };
                        p.best_price = if p_long { p.best_price.max(favorable) } else { p.best_price.min(favorable) };
                    }
                }
            }
            if let Some((e, reverse)) = exit {
                let p = assets[ai].pos.take().unwrap();
                if e.reason == "end" {
                    // Still open in reality: the levels it holds for the next bar, the trail
                    // ratcheted on this one now that it has closed.
                    let mut q = p.clone();
                    let own = if q.long { &assets[ai].long } else { &assets[ai].short };
                    if let Some(rule) = &own.trail {
                        ratchet(&mut q, rule, b, bar);
                    }
                    if forward && plan_next_open(&mut q, own, b, bar, &ex.exit_order, assets[ai].exit_atr.as_ref(), exit_valid) {
                        leaving_at_open.insert(b.ticker.to_string());
                    }
                    open_now.push(open_position(&q, own, b, bar, exit_through));
                }
                settle!(b, &p, e, bar);
                // Only a market exit at the open leaves that open available to a new entry (an
                // expired exit limit closing at the close carries the same reason, not the open).
                if !left_at_open {
                    assets[ai].exited_after_open = Some(bar);
                } else {
                    assets[ai].held_open = 0.0;
                    open_closes.extend(closed.last().cloned().map(|c| (assets[ai].t_open, c)));
                }
                // Stop-and-reverse: open the opposite side immediately at this bar's open.
                // The reverse entry sizes like a fresh one: risk-based sizing needs the
                // reverse side's stop price (from its SL rule + this bar's ATR), and equity
                // is marked-to-market across whatever other positions remain open.
                if reverse && (!is_last_bar || forward) {
                    let rlong = !p.long;
                    let c = fill.qcandle(b, bar);
                    let quoted = fill.quote(&c).is_some();
                    let px = fill.market_entry(&c, rlong);
                    let stop_px = first_stop_px(s, rlong, reverse_atr, px);
                    let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
                    let at = assets[ai].t_open;
                    let equity_now = mtm_equity(cash_open, &assets, at);
                    let margin_free = equity_now - committed_margin(&assets, mult, lev, at);
                    let known;
                    let ctx = SizeCtx { equity: equity_now, entry_px: px, stop_px, leverage: s.leverage, mult, closed: known_closed!(known, at) };
                    if let Some(lot) = make_lot(&s.sizing, &fees_taker, &s.instrument, mult, px, 1.0, margin_free, &ctx) {
                        cash -= lot.fee;
                        let mut np = Pos::new(rlong, lot, bar, reverse_atr, b.open[bar]);
                        np.arm_trail(if rlong { assets[ai].long.trail.as_ref() } else { assets[ai].short.trail.as_ref() });
                        assets[ai].pos = Some(np);
                        if assets[ai].pending.take().is_some() {
                            st.orders_expired += 1;
                        }
                        st.fills += 1;
                        st.quoted_fills += quoted as usize;
                        if let Some((p, e)) = entry_bar_exit(&mut assets[ai], bar, Start::Open, &fill, intrabar, exit_through, mult, &mut st) {
                            // It held its slot and its notional at this open all the same.
                            assets[ai].held_open = p.notional(assets[ai].mark, mult);
                            settle!(b, &p, e, bar);
                            assets[ai].exited_after_open = Some(bar);
                        }
                    }
                }
            }
        }

        // ── (2) Entries / pyramiding adds, asset by asset (submission order) ──
        {
            for &ai in &order {
                let Some(bar) = assets[ai].bar_at_row[r] else { continue };
                let at = assets[ai].t_open;
                if halted_at(&assets, at) {
                    continue;
                }
                let b = assets[ai].b;
                // No fill on the asset's final bar (nothing to hold into), unless the run is
                // forward: that bar has closed and the position lives on. A limit may still be
                // placed there: it is what a forward run has resting for the next bar.
                let final_bar = bar == b.ts.len() - 1;
                if final_bar && !entry_limit && !forward {
                    continue;
                }
                // Closed inside this bar: an entry at its open would predate that exit.
                if assets[ai].exited_after_open == Some(bar) {
                    continue;
                }
                // The bar with its bid/ask, read only when a market order fills on it.
                let market_candle = || {
                    let c = fill.qcandle(b, bar);
                    (c, fill.quote(&c).is_some())
                };

                // A resting order outlives its purpose when its position is gone (an add) or
                // was opened another way (an entry), when it expired, or when the window shut.
                let has_pos = assets[ai].pos.is_some();
                let stale = assets[ai].pending.as_ref().is_some_and(|pd| {
                    pd.add != has_pos || pd.last_bar < bar || (!window_open && (!pd.add || block_adds))
                });
                if stale {
                    assets[ai].pending = None;
                    st.orders_expired += 1;
                }

                // Sizing/limits evaluate against marked-to-market equity, and the margin
                // check against equity net of margin already committed by open positions —
                // recomputed per asset so earlier entries on this row are accounted for.
                let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
                let equity_now = mtm_equity(cash_open, &assets, at);
                let margin_free = equity_now - committed_margin(&assets, mult, lev, at);
                let known;
                let known_closed: &[ClosedTrade] = known_closed!(known, at);

                match &assets[ai].pos {
                    None => {
                        if !window_open || assets[ai].cold[bar] {
                            continue;
                        }
                        let long_fired = fired(&assets[ai].long.entry, bar);
                        let short_fired = fired(&assets[ai].short.entry, bar);
                        let want_long = if long_fired { Some(true) } else if short_fired { Some(false) } else { None };
                        if !entry_limit {
                            let Some(long) = want_long else { continue };
                            let (c, quoted) = market_candle();
                            // Filled on the live price at this open (paper): that price, as given.
                            let live = assets[ai].forced_entry.get(&bar).copied().filter(|f| f.0 == long);
                            if live.is_some() {
                                assets[ai].forced_entry.remove(&bar);
                            }
                            let px = live.map_or_else(|| fill.market_entry(&c, long), |f| f.1);
                            if !limits_allow_open(s, &assets, ai, px, mult, equity_now, false, at) {
                                continue;
                            }
                            let sig = if long { &assets[ai].long } else { &assets[ai].short };
                            // The last closed bar's ATR: the entry bar's own range is not known when it opens.
                            let entry_atr = sig.atr.as_ref().and_then(|v| bar.checked_sub(1).and_then(|k| v[k]));
                            let stop_px = first_stop_px(s, long, entry_atr, px);
                            let ctx = SizeCtx { equity: equity_now, entry_px: px, stop_px, leverage: s.leverage, mult, closed: known_closed };
                            // scale[0] for the first lot.
                            let scale0 = s.pyramid_steps.scale.first().copied().unwrap_or(1.0);
                            match make_lot_checked(&s.sizing, &fees_taker, &s.instrument, mult, px, scale0, margin_free, &ctx) {
                                LotOutcome::Ok(lot) => {
                                    if !exposure_allows_lot(s, &assets, ai, &lot, mult, equity_now, at) {
                                        skipped_exposure += 1;
                                        continue;
                                    }
                                    cash -= lot.fee;
                                    let mut np = Pos::new(long, lot, bar, entry_atr, b.open[bar]);
                                    np.arm_trail(sig.trail.as_ref());
                                    assets[ai].pos = Some(np);
                                    st.fills += 1;
                                    st.quoted_fills += quoted as usize;
                                    if let Some((p, e)) = entry_bar_exit(&mut assets[ai], bar, Start::Open, &fill, intrabar, exit_through, mult, &mut st) {
                                        // It held its slot and its notional at this open all the same.
                                        assets[ai].held_open = p.notional(assets[ai].mark, mult);
                                        settle!(b, &p, e, bar);
                                        assets[ai].exited_after_open = Some(bar);
                                    }
                                }
                                LotOutcome::BelowMin => skipped_min_size += 1,
                                LotOutcome::Margin => skipped_margin += 1,
                                LotOutcome::Zero => {}
                            }
                            continue;
                        }
                        // Limit entry: a signal places the order, priced off this bar. A signal that
                        // keeps holding does not re-price a resting order of the same direction.
                        let same = |pd: &PendingEntry| Some(pd.long) == want_long;
                        if let (Some(long), false) = (want_long, assets[ai].pending.as_ref().is_some_and(same)) {
                            let atr = assets[ai].entry_atr.as_ref();
                            if let Some(px) = limit_price(&ex.entry_order, b, bar - 1, bar, long, atr) {
                                let order = PendingEntry { long, add: false, px, last_bar: bar + entry_valid - 1 };
                                if assets[ai].pending.replace(order).is_some() {
                                    st.orders_expired += 1;
                                }
                                st.orders_placed += 1;
                            }
                        }
                        if final_bar && !forward {
                            continue;
                        }
                        let Some((long, px)) = assets[ai].pending.as_ref().map(|pd| (pd.long, pd.px)) else { continue };
                        // A fill the live price took replaces the candle's verdict on this bar.
                        let live = assets[ai].forced_entry.get(&bar).copied().filter(|f| f.0 == long);
                        if live.is_some() {
                            assets[ai].forced_entry.remove(&bar);
                        }
                        let filled = match live {
                            Some((_, lpx)) => Some((lpx, Start::Mid, false)),
                            None => fill.entry_limit_fill(b, bar, px, long, entry_through, intrabar),
                        };
                        let Some((fpx, start, fquoted)) = filled else { continue };
                        // Refused by a portfolio limit: the order keeps resting.
                        if !limits_allow_open(s, &assets, ai, fpx, mult, equity_now, false, at) {
                            continue;
                        }
                        assets[ai].pending = None;
                        let sig = if long { &assets[ai].long } else { &assets[ai].short };
                        // The last closed bar's ATR: the entry bar's own range is not known when it opens.
                        let entry_atr = sig.atr.as_ref().and_then(|v| bar.checked_sub(1).and_then(|k| v[k]));
                        let stop_px = first_stop_px(s, long, entry_atr, fpx);
                        let ctx = SizeCtx { equity: equity_now, entry_px: fpx, stop_px, leverage: s.leverage, mult, closed: known_closed };
                        let scale0 = s.pyramid_steps.scale.first().copied().unwrap_or(1.0);
                        match make_lot_checked(&s.sizing, &fees_maker, &s.instrument, mult, fpx, scale0, margin_free, &ctx) {
                            LotOutcome::Ok(lot) => {
                                if !exposure_allows_lot(s, &assets, ai, &lot, mult, equity_now, at) {
                                    skipped_exposure += 1;
                                    st.orders_expired += 1;
                                    continue;
                                }
                                cash -= lot.fee;
                                let mut np = Pos::new(long, lot, bar, entry_atr, fpx);
                                np.arm_trail(sig.trail.as_ref());
                                assets[ai].pos = Some(np);
                                st.orders_filled += 1;
                                st.fills += 1;
                                st.quoted_fills += fquoted as usize;
                                if let Some((p, e)) = entry_bar_exit(&mut assets[ai], bar, start, &fill, intrabar, exit_through, mult, &mut st) {
                                    // It held its slot and its notional all the same.
                                    assets[ai].held_open = p.notional(assets[ai].mark, mult);
                                    settle!(b, &p, e, bar);
                                    assets[ai].exited_after_open = Some(bar);
                                }
                            }
                            other => {
                                st.orders_expired += 1;
                                match other {
                                    LotOutcome::BelowMin => skipped_min_size += 1,
                                    LotOutcome::Margin => skipped_margin += 1,
                                    _ => {}
                                }
                            }
                        }
                    }
                    // Adds are sent in the manage pass, before the bar's levels are tested.
                    Some(_) => {}
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
            let Some(long) = a.pos.as_ref().map(|p| p.long) else { continue };
            let Some(rule) = (if long { &a.long.trail } else { &a.short.trail }).clone() else { continue };
            let b = a.b;
            ratchet(a.pos.as_mut().unwrap(), &rule, b, bar);
        }

        // ── (3b) Forward run: a position opened at the open of the asset's last bar ──
        // The manage pass already closed what was open before; this one came after it. It is
        // held into the next bar, so it is reported as open, with its trail already ratcheted.
        if forward {
            for a in assets.iter_mut() {
                let Some(bar) = a.bar_at_row[r] else { continue };
                let b = a.b;
                if bar != b.ts.len() - 1 {
                    continue;
                }
                let Some(mut p) = a.pos.take() else { continue };
                let own = if p.long { &a.long } else { &a.short };
                if plan_next_open(&mut p, own, b, bar, &ex.exit_order, a.exit_atr.as_ref(), exit_valid) {
                    leaving_at_open.insert(b.ticker.to_string());
                }
                open_now.push(open_position(&p, own, b, bar, exit_through));
                let c = fill.qcandle(b, bar);
                let px = fill.market_exit(b.close[bar], c.q, true, p.long);
                settle!(b, &p, ExitFill { px, reason: "end", maker: false, quoted: fill.quote(&c).is_some() }, bar);
            }
        }

        // ── (3c) A hole in the data follows this bar ──
        // Whatever is still open cannot be carried through it: the price on the far side is
        // not a price the position lived through. It leaves at this close, outside the book,
        // and a resting order is cancelled with it.
        for a in assets.iter_mut() {
            let Some(bar) = a.bar_at_row[r] else { continue };
            if !a.gap_after[bar] {
                continue;
            }
            if a.pending.take().is_some() {
                st.orders_expired += 1;
            }
            let Some(p) = a.pos.take() else { continue };
            let b = a.b;
            let c = fill.qcandle(b, bar);
            let px = fill.market_exit(b.close[bar], c.q, true, p.long);
            settle!(b, &p, ExitFill { px, reason: "data_gap", maker: false, quoted: false }, bar);
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

    st.forced_unmatched = assets.iter().map(|a| a.forced_exit.len() + a.forced_entry.len()).sum();

    // The size an order sent now would fill with at `px`, sized like the engine sizes it, on
    // the equity marked at the last close (the positions still held, not closed on paper) and
    // the margin they commit. None when the sizing refuses it.
    let lev = if s.leverage > 0.0 { s.leverage } else { 1.0 };
    let equity_marked = cash + end_cost;
    // Margin the open positions commit at the last close, except one that leaves at the next
    // open (`freed`): its margin is free again when the new order fills.
    let margin_free = |freed: Option<&str>| {
        let held: f64 = open_now
            .iter()
            .filter(|p| Some(p.ticker.as_str()) != freed)
            .map(|p| {
                let close = inputs.iter().find(|b| b.ticker == p.ticker).and_then(|b| b.close.last().copied()).unwrap_or(p.avg_price);
                p.qty.abs() * close.abs() * mult
            })
            .sum();
        equity_marked - held / lev
    };
    let expected_qty = |long: bool, px: f64, maker: bool, atr: Option<f64>, freed: Option<&str>| {
        let margin_free = margin_free(freed);
        let ctx = SizeCtx { equity: equity_marked, entry_px: px, stop_px: first_stop_px(s, long, atr, px), leverage: s.leverage, mult, closed: &closed };
        let scale0 = s.pyramid_steps.scale.first().copied().unwrap_or(1.0);
        match make_lot_checked(&s.sizing, fees_of(maker), &s.instrument, mult, px, scale0, margin_free, &ctx) {
            LotOutcome::Ok(lot) => Some(lot.qty),
            _ => None,
        }
    };

    // Entry limits still resting after each asset's last bar.
    for a in &assets {
        let last = a.b.ts.len().saturating_sub(1);
        if let Some(pd) = a.pending.as_ref().filter(|pd| pd.last_bar >= last) {
            let sig = if pd.long { &a.long } else { &a.short };
            working.push(WorkingOrder {
                ticker: a.b.ticker.to_string(),
                kind: if pd.add { "add".into() } else { "entry".into() },
                direction: if pd.long { "long".into() } else { "short".into() },
                price: pd.px,
                bars_left: pd.last_bar - last + 1,
                order: "limit".into(),
                qty: (!pd.add).then(|| expected_qty(pd.long, pd.px, true, sig.atr.as_ref().and_then(|v| v[last]), None)).flatten(),
                amount: (!pd.add)
                    .then(|| expected_qty(pd.long, pd.px, true, sig.atr.as_ref().and_then(|v| v[last]), None))
                    .flatten()
                    .map(|q| q * pd.px * mult),
                ..WorkingOrder::default()
            });
        }
    }

    // Forward runs: the entry the last close decided, which the next bar sends. A paper session
    // announces it at that close and fills it at the next open.
    if forward {
        for a in &assets {
            let b = a.b;
            let Some(last) = b.ts.len().checked_sub(1) else { continue };
            // A position still held only makes room at the next open when it leaves there: on a
            // market signal exit or an expired exit limit, or reversed (a market flip).
            let mut reverse = false;
            if let Some(p) = open_now.iter().find(|p| p.ticker == b.ticker) {
                let long = p.direction == "long";
                reverse = s.stop_and_reverse && fired(if long { &a.short.entry } else { &a.long.entry }, last + 1);
                if !(leaving_at_open.contains(b.ticker) || reverse) {
                    continue;
                }
            }
            let want = if fired(&a.long.entry, last + 1) {
                true
            } else if fired(&a.short.entry, last + 1) {
                false
            } else {
                continue;
            };
            let entry_limit = entry_limit && !reverse;
            if entry_limit && a.pending.as_ref().is_some_and(|pd| pd.long == want && pd.last_bar > last) {
                continue; // the same order is already resting
            }
            if breaker_halted(&s.risk, cash, peak, day_start_equity) || !next_bar_open(&s.filters, b.ts) {
                continue;
            }
            let sig = if want { &a.long } else { &a.short };
            let atr = sig.atr.as_ref().and_then(|v| v[last]);
            let close = b.close[last];
            let spec = &ex.entry_order;
            let provisional = entry_limit && spec.reference == "open";
            let pct = spec.offset_kind == "pct";
            let mut anchor_offset = None;
            let price = if entry_limit {
                let lim_atr = a.entry_atr.as_ref().and_then(|v| v[last]);
                let Some(d) = spec.distance(close, lim_atr) else { continue };
                if provisional {
                    anchor_offset = Some(if pct { spec.offset } else { d });
                }
                if want { close - d } else { close + d }
            } else {
                close
            };
            let fill_px = if entry_limit { price } else { fill.entry(close, want) };
            let freed = open_now.iter().any(|p| p.ticker == b.ticker).then_some(b.ticker);
            let Some(qty) = expected_qty(want, fill_px, entry_limit, atr, freed) else { continue };
            working.push(WorkingOrder {
                ticker: b.ticker.to_string(),
                kind: "entry".into(),
                direction: if want { "long".into() } else { "short".into() },
                price,
                bars_left: if entry_limit { entry_valid } else { 1 },
                order: if entry_limit { "limit".into() } else { "market".into() },
                signal_ts: Some(b.ts[last].clone()),
                qty: Some(qty),
                amount: Some(qty * fill_px * mult),
                atr,
                provisional,
                anchor_offset,
                anchor_pct: provisional && pct,
            });
        }
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
        execution: st,
        pending_orders: working,
        open_positions: open_now,
        data_gaps: data_gaps(inputs),
        excluded_trades: excluded,
    }
}

/// Bars every input of `s` needs to be whole again after a hole: the longest indicator, and
/// the ATR periods its stops and orders read.
fn hole_warmup(s: &Settings) -> usize {
    let stops = [s.long.as_ref(), s.short.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|sd| [sd.sl_rule(), sd.tp_rule()])
        .flatten()
        .filter(|st| st.is_atr())
        .map(|st| if st.period == 0 { 14 } else { st.period });
    let orders = [s.execution.entry_order.atr_period(), s.execution.exit_order.atr_period()].into_iter().flatten();
    stops.chain(orders).fold(warmup_bars(s), usize::max)
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
    let cfg = s.grid.clone().unwrap_or_default();
    // No band given (no `grid`, or a fixed ladder over [0, 0]): the ladder spans the range known
    // so far, the lowest low to the highest high of the bars before each one. Never the range of
    // the whole series, which no one trading bar i could have known.
    let known_range = !(!cfg.anchor.is_empty() && cfg.anchor != "none") && cfg.lower == 0.0 && cfg.upper == 0.0;

    let levels = cfg.levels.clamp(2, 200);
    let (lo, hi) = (cfg.lower.min(cfg.upper), cfg.lower.max(cfg.upper));
    let step = if levels > 1 { (hi - lo) / (levels - 1) as f64 } else { 0.0 };
    let is_short = cfg.direction == "short";
    // Neutral: the cells below the ladder's centre line trade long, the ones above it short, so
    // the grid trades both sides of its anchor (the centre is a line: `validate` wants an odd
    // number of levels for it).
    let neutral = cfg.direction == "neutral";
    let tick = s.instrument.tick_size.max(0.0);
    let cells = levels.saturating_sub(1).max(1);

    // The ladder as (bottom line, spacing) per bar. A fixed grid repeats the same pair on every
    // bar; an anchored grid re-centres it on the reference line, and stays None while the
    // reference (or the ATR backing its width) is still warming up.
    let anchored = !cfg.anchor.is_empty() && cfg.anchor != "none";
    let (mut lo_at, mut step_at) = (vec![Some(lo); n], vec![step; n]);
    if known_range {
        let (mut lo_k, mut hi_k) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..n {
            let ok = i > 0 && hi_k > lo_k;
            lo_at[i] = ok.then_some(lo_k);
            step_at[i] = if ok { (hi_k - lo_k) / (levels - 1) as f64 } else { 0.0 };
            lo_k = lo_k.min(b.low[i]);
            hi_k = hi_k.max(b.high[i]);
        }
    } else if anchored {
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
    // Holes in the data: inventory is not carried through one, and no cell opens until the
    // ladder's reference has warmed up again on the far side.
    let grid_holes = holes::find(b.ts);
    let gap_after = holes::before_mask(n, &grid_holes);
    let ladder_warmup = if anchored {
        let width = if cfg.width_kind == "atr" { cfg.width_period.clamp(1, 5_000) } else { 0 };
        cfg.anchor_period.clamp(1, 5_000).max(width)
    } else {
        0
    };
    let cold = holes::cold_mask(n, &grid_holes, ladder_warmup);
    let mut excluded: Vec<Trade> = Vec::new();

    // A grid works with resting limit orders: both legs of a cell fill AT their line, so they
    // cross no spread and take no slippage. Only a forced exit is a market order (the
    // circuit-stop liquidation, and the close of a `reset_on_close` bar), and that one pays
    // the spread like any market fill.
    let buy_px = |raw: f64| on_tick(raw * (1.0 + half_spread), tick, true);
    let sell_px = |raw: f64| on_tick(raw * (1.0 - half_spread), tick, false);
    let short_of = |c: usize| is_short || (neutral && c >= cells / 2);

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
                  reason: &str,
                  is_short: bool| {
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
            let (o, hi, lo) = (b.open[i], b.high[i], b.low[i]);
            // Window closed with "flat": the inventory goes at this bar's open, before anything
            // else in the bar, like the signal engine's session_end exit.
            if !window_open && flat_on_close {
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, o, true, "session_end", short_of(c));
                        fills += 1;
                    }
                }
            }
            // Where the price stood when this bar's ladder was put out: the last close (this
            // bar's open on the first). A buy rests on a line below it, a sell on a line above;
            // a line on the wrong side of the market is not a resting order.
            let reference = if i > 0 { b.close[i - 1] } else { o };
            // A cell's entry: the line, or the open when the bar opened through it.
            // Returns (line, fill, target): a buy line on the tick below, a sell line on the tick
            // above, the target a tick further away, a gap fill on the open's adverse tick.
            let entry_fill = |c: usize| -> Option<(f64, f64, f64)> {
                let is_short = short_of(c);
                let raw = line_at(i, if is_short { c + 1 } else { c })?;
                let line = on_tick(raw, tick, is_short);
                let target = on_tick(if is_short { raw - step_at[i] } else { raw + step_at[i] }, tick, !is_short);
                let resting = if is_short { line > reference } else { line < reference };
                let px = if is_short { on_tick(o, tick, false).max(line) } else { on_tick(o, tick, true).min(line) };
                resting.then_some((line, px, target))
            };
            // Circuit stops: the first one the bar reaches liquidates everything and halts. A
            // bar that opens past one fills there; otherwise at the stop, after the fills the
            // price met on its way from the open to it. Both in range: the one the bar opened
            // past, else (order unknown) the one against the grid.
            let below = cfg.stop_below > 0.0 && lo <= cfg.stop_below;
            let above = cfg.stop_above > 0.0 && hi >= cfg.stop_above;
            let down = match (below, above) {
                (true, true) if o <= cfg.stop_below => true,
                (true, true) if o >= cfg.stop_above => false,
                (true, true) => !is_short,
                _ => below,
            };
            if below || above {
                let stop = if down { cfg.stop_below } else { cfg.stop_above };
                let gapped = if down { o <= stop } else { o >= stop };
                let exit_px = if gapped { o } else { stop };
                if !gapped {
                    // The path open → stop crosses every level between them, in that direction.
                    let on_path = |x: f64| if down { x >= stop && x <= o } else { x <= stop && x >= o };
                    for c in 0..cells {
                        match held[c] {
                            // A long grid buys on the way down, a short grid sells on the way up.
                            None if window_open && !cold[i] && down != short_of(c) => {
                                if let Some((line, px, target)) = entry_fill(c) {
                                    if on_path(line) || px != line {
                                        held[c] = Some((i, px, target));
                                        fills += 1;
                                        cash -= fee_for(&s.fees, qty, px, mult);
                                    }
                                }
                            }
                            // Targets are met on the way up (long) or down (short).
                            Some((entry_bar, entry_raw, target)) if down == short_of(c) && on_path(target) => {
                                held[c] = None;
                                settle(&mut trades, &mut cash, entry_bar, i, entry_raw, target, false, "take_profit", short_of(c));
                                fills += 1;
                                round_trips += 1;
                            }
                            _ => {}
                        }
                    }
                }
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, exit_px, true, "stop_loss", short_of(c));
                    }
                }
                halted = true;
            } else {
                // For each cell, entry when the bar reaches a resting buy level, exit one step away.
                for c in 0..cells {
                    match held[c] {
                        None => {
                            if !window_open || cold[i] {
                                continue;
                            }
                            let Some((line, px, target)) = entry_fill(c) else { continue };
                            let hit = if short_of(c) { hi >= line } else { lo <= line };
                            if hit {
                                held[c] = Some((i, px, target));
                                fills += 1;
                                // Entry fee taken on fill, on the price it filled at.
                                cash -= fee_for(&s.fees, qty, px, mult);
                            }
                        }
                        Some((entry_bar, entry_raw, target)) => {
                            // Exit fills when price reaches the lot's own target → round trip, at
                            // the open when the bar opened past it.
                            let is_short = short_of(c);
                            let hit = if is_short { lo <= target } else { hi >= target };
                            if hit {
                                let px = if is_short { on_tick(o, tick, true).min(target) } else { on_tick(o, tick, false).max(target) };
                                held[c] = None;
                                settle(&mut trades, &mut cash, entry_bar, i, entry_raw, px, false, "take_profit", short_of(c));
                                fills += 1;
                                round_trips += 1;
                            }
                        }
                    }
                }
            }

            // Retrigger: flatten on the close so the next bar trades a ladder centred on the new
            // anchor. Settled cells count as fills, not as round trips: they are flushed, not
            // taken profit on.
            if cfg.reset_on_close && !halted {
                for c in 0..cells {
                    if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                        settle(&mut trades, &mut cash, entry_bar, i, entry_raw, b.close[i], true, "grid_reset", short_of(c));
                        fills += 1;
                    }
                }
            }
        }

        // A hole follows this bar: the inventory leaves at this close, outside the book. Its
        // entry fee, taken at the fill, goes back: the trade did not happen as far as the
        // account knows.
        if gap_after[i] {
            for c in 0..cells {
                if let Some((entry_bar, entry_raw, _)) = held[c].take() {
                    let mut void_cash = 0.0;
                    settle(&mut excluded, &mut void_cash, entry_bar, i, entry_raw, b.close[i], true, "data_gap", short_of(c));
                    cash += fee_for(&s.fees, qty, entry_raw, mult);
                }
            }
        }

        // Mark-to-market: cash + open inventory valued at this close.
        let mut inv_units = 0.0;
        let mut inv_val = 0.0;
        for c in 0..cells {
            if let Some((_, entry_raw, _)) = held[c] {
                let dir = if short_of(c) { -1.0 } else { 1.0 };
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
    let dir_of = |c: usize| if short_of(c) { -1.0 } else { 1.0 };
    let end_units: f64 = (0..cells).filter(|&c| held[c].is_some()).map(|c| dir_of(c) * qty).sum();
    let end_val = {
        let mut v = 0.0;
        for c in 0..cells {
            if let Some((_, entry_raw, _)) = held[c] {
                v += dir_of(c) * qty * (b.close[n - 1] - entry_raw) * mult;
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
            long: side_signals(None, false, b, &s.indicators, &s.execution, 0.0),
            short: side_signals(None, false, b, &s.indicators, &s.execution, 0.0),
            bar_at_row: (0..n).map(Some).collect(),
            pos: None,
            active_pos_rows: 0,
            last_close: b.close.first().copied().unwrap_or(0.0),
            mark: b.open.first().copied().unwrap_or(0.0),
            held_open: 0.0,
            epochs: Vec::new(),
            t_open: None,
            open_unreal: 0.0,
            pre_unreal: 0.0,
            pre_notional: 0.0,
            pre_held: false,
            pending: None,
            entry_atr: None,
            exit_atr: None,
            exited_after_open: None,
            forced_exit: HashMap::new(),
            forced_entry: HashMap::new(),
            gap_after: Vec::new(),
            cold: Vec::new(),
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
        execution: ExecStats::default(),
        pending_orders: Vec::new(),
        open_positions: Vec::new(),
        data_gaps: data_gaps(&[b]),
        excluded_trades: excluded,
    }
}


/// Equal-weight buy-and-hold equity over the merged clock: split the starting capital across the
/// assets, buy each at its first available (spread-free) close minus one entry fee, hold to the
/// end. Marked-to-market at each asset's last-known price each row. Cash-neutral for assets that
/// haven't started yet (their slice stays as cash). A fair "did the strategy beat holding?" line.
fn buy_hold_curve(s: &Settings, clock: &[String], assets: &[Asset], mult: f64) -> Vec<EquityPoint> {
    // Held only across the calendar window, so the line ends on the same number the KPI shows:
    // cash before the window opens, frozen at the last in-window price after it closes.
    let (w0, w1) = s.filters.date_window(clock).unwrap_or((0, clock.len().saturating_sub(1)));
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
            let live = r >= w0 && r <= w1;
            for (ai, a) in assets.iter().enumerate() {
                if let Some(bar) = a.bar_at_row[r].filter(|_| live) {
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
struct FillModel {
    half_spread: f64,
    slippage: Slippage,
    /// Price off a candle's own bid/ask when it has them (`execution.use_quotes`).
    quotes: bool,
    /// Instrument tick (0 = none): fills and levels are rounded onto it, against the trader.
    tick: f64,
}

/// The open of `c` alone, as a candle that did nothing else: what a level the bar opened beyond
/// does before anything inside the bar.
fn open_only(c: Candle) -> Candle {
    let q = c.q.map(|q| Quote {
        bid_high: q.bid_open,
        bid_low: q.bid_open,
        bid_close: q.bid_open,
        ask_high: q.ask_open,
        ask_low: q.ask_open,
        ask_close: q.ask_open,
        ..q
    });
    Candle { o: c.o, h: c.o, l: c.o, q }
}

/// One candle of a path: the strategy bar itself, or one lower-timeframe bar inside it.
#[derive(Clone, Copy)]
struct Candle {
    o: f64,
    h: f64,
    l: f64,
    q: Option<Quote>,
}

/// A bar's mid (or trade) candle, without its bid/ask.
fn candle(b: &Bars, i: usize) -> Candle {
    Candle { o: b.open[i], h: b.high[i], l: b.low[i], q: None }
}

/// How close to a bar's range a level has to be for the bar's bid/ask to be worth reading: a
/// spread wider than this is not one a level-based exit is decided by.
const QUOTE_REACH: f64 = 0.005;

fn sub_candle(sb: &SubSlice, k: usize) -> Candle {
    Candle { o: sb.open[k], h: sb.high[k], l: sb.low[k], q: sb.quotes.as_ref().and_then(|q| q[k]) }
}

/// A fill an exit produced: the final price (spread and slippage already in), why, whether it
/// was a resting limit (maker fee) and whether real bid/ask priced it.
#[derive(Clone, Copy)]
struct ExitFill {
    px: f64,
    reason: &'static str,
    maker: bool,
    quoted: bool,
}

/// The exit levels a position holds through one bar. `limits` are resting sells above a long
/// (buys below a short): the take profit and a pending exit limit.
struct Levels {
    long: bool,
    stop: Option<(f64, &'static str)>,
    limits: Vec<(f64, &'static str, bool)>,
}

/// What one candle did to a position's levels.
enum Scan {
    Quiet,
    Exit(ExitFill),
    /// The stop and a limit were both reached and the candle cannot say which came first.
    /// Carries the worst case: the stop.
    Both(ExitFill),
    /// Only a limit was reached, in the candle a limit entry filled mid-way: the target may
    /// have printed before the position existed. Worst case: no exit.
    Maybe,
}

/// Where a position's first bar starts: at the bar's open (a market entry, a gap fill), at an
/// unknown point inside it (a limit filled mid-bar, no lower timeframe), or inside lower-timeframe
/// bar `k` (at its open when the flag is set).
#[derive(Clone, Copy)]
enum Start {
    Open,
    Mid,
    Sub(usize, bool),
}

impl FillModel {
    fn new(s: &Settings) -> FillModel {
        let tick = s.instrument.tick_size.max(0.0);
        let mut slippage = s.slippage.clone();
        // Slippage in ticks with no tick of its own counts the instrument's.
        if slippage.tick_size <= 0.0 {
            slippage.tick_size = tick;
        }
        FillModel { half_spread: s.spread_pct / 2.0, slippage, quotes: s.execution.use_quotes, tick }
    }
    /// An execution price on the tick grid, against the trader: a buy pays the tick above.
    fn deal(&self, px: f64, buy: bool) -> f64 {
        on_tick(px, self.tick, buy)
    }
    fn quote(&self, c: &Candle) -> Option<Quote> {
        if self.quotes { c.q } else { None }
    }
    /// Bar `i` with its bid/ask when the run prices on them.
    fn qcandle(&self, b: &Bars, i: usize) -> Candle {
        let mut c = candle(b, i);
        if self.quotes {
            c.q = b.quotes.and_then(|q| q.quote(i));
        }
        c
    }
    /// Bar `i` with its bid/ask only when one of `levels` is within reach of its range: a bar
    /// far from every level fills nothing, whatever its spread.
    fn qcandle_near(&self, b: &Bars, i: usize, levels: impl IntoIterator<Item = f64>) -> Candle {
        let (lo, hi) = (b.low[i] * (1.0 - QUOTE_REACH), b.high[i] * (1.0 + QUOTE_REACH));
        if levels.into_iter().any(|x| x >= lo && x <= hi) {
            self.qcandle(b, i)
        } else {
            candle(b, i)
        }
    }
    /// The price a buyer pays at the open / high / low of the candle, before slippage.
    fn ask(&self, c: &Candle) -> (f64, f64, f64) {
        match self.quote(c) {
            Some(q) => (q.ask_open, q.ask_high, q.ask_low),
            None => {
                let k = 1.0 + self.half_spread;
                (c.o * k, c.h * k, c.l * k)
            }
        }
    }
    /// The price a seller receives at the open / high / low of the candle, before slippage.
    fn bid(&self, c: &Candle) -> (f64, f64, f64) {
        match self.quote(c) {
            Some(q) => (q.bid_open, q.bid_high, q.bid_low),
            None => {
                let k = 1.0 - self.half_spread;
                (c.o * k, c.h * k, c.l * k)
            }
        }
    }
    /// A market entry at the candle's open: the ask (long) or the bid (short), plus slippage.
    fn market_entry(&self, c: &Candle, long: bool) -> f64 {
        match self.quote(c) {
            Some(q) => {
                let base = if long { q.ask_open } else { q.bid_open };
                let slip = self.slippage.amount(base);
                self.deal(if long { base + slip } else { base - slip }, long)
            }
            None => self.entry(c.o, long),
        }
    }
    /// A market exit at `mid` (an open or a close), on the side that deals.
    fn market_exit(&self, mid: f64, q: Option<Quote>, at_close: bool, long: bool) -> f64 {
        match q.filter(|_| self.quotes) {
            Some(q) => {
                let base = match (long, at_close) {
                    (true, false) => q.bid_open,
                    (true, true) => q.bid_close,
                    (false, false) => q.ask_open,
                    (false, true) => q.ask_close,
                };
                let slip = self.slippage.amount(base);
                self.deal(if long { base - slip } else { base + slip }, !long)
            }
            None => self.exit(mid, long),
        }
    }
    /// A stop exit if the candle reached `stop`. From the open, a candle that opens beyond the
    /// stop fills at the open (the gap rule); mid-candle, the stop price is the best case.
    fn stop_exit(&self, c: &Candle, stop: f64, long: bool, from_open: bool, reason: &'static str) -> Option<ExitFill> {
        match self.quote(c) {
            Some(q) => {
                let (open, extreme) = if long { (q.bid_open, q.bid_low) } else { (q.ask_open, q.ask_high) };
                let reached = if long { extreme <= stop } else { extreme >= stop };
                if !reached {
                    return None;
                }
                let base = match (from_open, long) {
                    (true, true) => stop.min(open),
                    (true, false) => stop.max(open),
                    _ => stop,
                };
                let slip = self.slippage.amount(base);
                Some(ExitFill { px: self.deal(if long { base - slip } else { base + slip }, !long), reason, maker: false, quoted: true })
            }
            None => {
                let reached = if long { c.l <= stop } else { c.h >= stop };
                if !reached {
                    return None;
                }
                let raw = match (from_open, long) {
                    (true, true) => stop.min(c.o),
                    (true, false) => stop.max(c.o),
                    _ => stop,
                };
                Some(ExitFill { px: self.exit(raw, long), reason, maker: false, quoted: false })
            }
        }
    }
    /// Fill of a resting limit over one candle, or None. `buy` = a buy limit at or below `px`.
    /// A candle that opens through the limit fills at its open (the better price); otherwise
    /// at the limit. No slippage: a resting order is filled, it does not chase.
    fn limit_fill(&self, c: &Candle, px: f64, buy: bool, through: bool, from_open: bool) -> Option<(f64, bool)> {
        let beyond = |x: f64| match (buy, through) {
            (true, false) => x <= px,
            (true, true) => x < px,
            (false, false) => x >= px,
            (false, true) => x > px,
        };
        let (open, high, low) = if buy { self.ask(c) } else { self.bid(c) };
        if from_open && beyond(open) {
            // Through the limit at the open: filled there, on the tick against the trader,
            // never past the limit itself.
            let at = self.deal(open, buy);
            return Some((if buy { at.min(px) } else { at.max(px) }, true));
        }
        beyond(if buy { low } else { high }).then_some((px, false))
    }
    /// What `c` does to `lv`. From the open, a gap past a level fills there first; past the
    /// open, the stop and a limit both reached is ambiguous. `after_fill`: the position was
    /// filled mid-candle by a limit, so a limit target here may predate it.
    fn scan(&self, c: &Candle, lv: &Levels, from_open: bool, after_fill: bool) -> Scan {
        let quoted = self.quote(c).is_some();
        let stop = lv.stop.and_then(|(px, reason)| self.stop_exit(c, px, lv.long, from_open, reason));
        // The nearest resting limit is the one the market reaches first.
        let mut nearest: Option<(f64, f64, bool, &'static str)> = None;
        for &(px, reason, through) in &lv.limits {
            if let Some((fill, at_open)) = self.limit_fill(c, px, !lv.long, through, from_open) {
                let nearer = nearest.is_none_or(|(lvl, ..)| if lv.long { px < lvl } else { px > lvl });
                if nearer {
                    nearest = Some((px, fill, at_open, reason));
                }
            }
        }
        let limit_fill =
            nearest.map(|(_, px, at_open, reason)| (ExitFill { px, reason, maker: true, quoted }, at_open));
        match (stop, limit_fill) {
            (None, None) => Scan::Quiet,
            // A gap through a limit fills at the open, before anything else in the candle.
            (_, Some((l, true))) => Scan::Exit(l),
            (Some(s), Some(_)) => {
                let gapped = from_open && {
                    let open = if lv.long { self.bid(c).0 } else { self.ask(c).0 };
                    lv.stop.is_some_and(|(px, _)| if lv.long { open <= px } else { open >= px })
                };
                if gapped { Scan::Exit(s) } else { Scan::Both(s) }
            }
            (Some(s), None) => Scan::Exit(s),
            (None, Some((l, _))) => {
                if after_fill { Scan::Maybe } else { Scan::Exit(l) }
            }
        }
    }
    /// The exit `lv` produces in bar `bar` of `b`, resolving an ambiguous bar on the lower
    /// timeframe when `intrabar` is on and it covers the bar, else by the worst case.
    fn bar_exit(&self, b: &Bars, bar: usize, lv: &Levels, start: Start, intrabar: bool, st: &mut ExecStats) -> Option<ExitFill> {
        let levels = lv.stop.map(|(x, _)| x).into_iter().chain(lv.limits.iter().map(|l| l.0));
        let parent = self.qcandle_near(b, bar, levels);
        let first = match start {
            Start::Open => self.scan(&parent, lv, true, false),
            Start::Mid | Start::Sub(..) => self.scan(&parent, lv, false, true),
        };
        let worst = match first {
            Scan::Quiet => return None,
            // A certain exit needs no lower timeframe, except after a fill located inside it:
            // the stop is then priced off the sub-bars it actually crossed.
            Scan::Exit(e) if !matches!(start, Start::Sub(..)) => return Some(e),
            Scan::Exit(e) => Some(e),
            Scan::Both(e) => {
                st.ambiguous_bars += 1;
                Some(e)
            }
            Scan::Maybe => {
                st.ambiguous_bars += 1;
                None
            }
        };
        let ambiguous = !matches!(first, Scan::Exit(_));
        let want = ambiguous || matches!(start, Start::Sub(..));
        let subs = if intrabar && want { sub_slice(b, bar) } else { None };
        let Some(sb) = subs else {
            if ambiguous {
                st.unresolved_bars += 1;
            }
            return worst;
        };
        let to = sb.high.len();
        let (k0, mut from_open, mut after_fill) = match start {
            Start::Open => (0, true, false),
            Start::Sub(k, at_open) => (k, at_open, !at_open),
            Start::Mid => {
                if ambiguous {
                    st.unresolved_bars += 1;
                }
                return worst;
            }
        };
        for k in k0..to {
            match self.scan(&sub_candle(&sb, k), lv, from_open, after_fill) {
                Scan::Quiet | Scan::Maybe => {}
                Scan::Exit(e) => {
                    if ambiguous {
                        st.resolved_bars += 1;
                    }
                    return Some(e);
                }
                Scan::Both(e) => {
                    if ambiguous {
                        st.unresolved_bars += 1;
                    }
                    return Some(e);
                }
            }
            from_open = true;
            after_fill = false;
        }
        // The lower timeframe never reached what the bar did: it does not describe this bar.
        if ambiguous {
            match worst {
                Some(_) => st.unresolved_bars += 1,
                None => st.resolved_bars += 1,
            }
        }
        worst
    }
    /// The exit `lv` takes in the lower-timeframe candles of `bar` before candle `k`, if any: what
    /// happened before a limit add filled inside candle `k`.
    fn exit_before_sub(&self, b: &Bars, bar: usize, lv: &Levels, k: usize) -> Option<ExitFill> {
        let sb = sub_slice(b, bar)?;
        for j in 0..k.min(sb.high.len()) {
            match self.scan(&sub_candle(&sb, j), lv, true, false) {
                Scan::Exit(e) | Scan::Both(e) => return Some(e),
                Scan::Quiet | Scan::Maybe => {}
            }
        }
        None
    }
    /// Where in bar `bar` a resting entry limit at `px` fills, if it does: the fill price, the
    /// start of the position's first bar, and whether real bid/ask priced it.
    fn entry_limit_fill(&self, b: &Bars, bar: usize, px: f64, long: bool, through: bool, intrabar: bool) -> Option<(f64, Start, bool)> {
        // A buy limit rests on the tick below its price, a sell limit on the tick above.
        let px = on_tick(px, self.tick, !long);
        let parent = self.qcandle_near(b, bar, [px]);
        let (fill, at_open) = self.limit_fill(&parent, px, long, through, true)?;
        let quoted = self.quote(&parent).is_some();
        if at_open {
            return Some((fill, Start::Open, quoted));
        }
        if intrabar {
            if let Some(sb) = sub_slice(b, bar) {
                for k in 0..sb.high.len() {
                    let c = sub_candle(&sb, k);
                    if let Some((f, open)) = self.limit_fill(&c, px, long, through, true) {
                        return Some((f, Start::Sub(k, open), self.quote(&c).is_some()));
                    }
                }
            }
        }
        Some((fill, Start::Mid, quoted))
    }
}

/// The lower-timeframe candles under bar `bar`, when the source has them and they agree with
/// the bar: their extremes match the bar's within 5 bp. Candles that saw prices the bar did not,
/// or missed the bar's own extreme, describe some other series and are not used.
fn sub_slice(b: &Bars, bar: usize) -> Option<std::sync::Arc<SubSlice>> {
    let sb = b.sub?.candles(bar)?;
    if sb.high.is_empty() || sb.high.len() != sb.low.len() || sb.high.len() != sb.open.len() {
        return None;
    }
    let hi = sb.high.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lo = sb.low.iter().copied().fold(f64::INFINITY, f64::min);
    let tol = b.high[bar].abs().max(b.low[bar].abs()) * 5e-4;
    ((hi - b.high[bar]).abs() <= tol && (lo - b.low[bar]).abs() <= tol).then_some(sb)
}

impl FillModel {
    /// Entry fill price (long pays up, short sells down).
    fn entry(&self, raw: f64, long: bool) -> f64 {
        let base = if long { raw * (1.0 + self.half_spread) } else { raw * (1.0 - self.half_spread) };
        let slip = self.slippage.amount(base);
        self.deal(if long { base + slip } else { base - slip }, long)
    }
    /// Exit fill price (long sells down, short buys up).
    fn exit(&self, raw: f64, long: bool) -> f64 {
        let base = if long { raw * (1.0 - self.half_spread) } else { raw * (1.0 + self.half_spread) };
        let slip = self.slippage.amount(base);
        self.deal(if long { base - slip } else { base + slip }, !long)
    }
}

/// The stop price for a *fresh* lot (first entry) — from the side's SL rule, sampling ATR when
/// the rule is ATR-based. Used by risk-based sizing at entry time (before a Pos exists).
fn first_stop_px(s: &Settings, long: bool, entry_atr: Option<f64>, entry: f64) -> Option<f64> {
    let side = (if long { s.long.as_ref() } else { s.short.as_ref() })?;
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
    inst: &Instrument,
    mult: f64,
    px: f64,
    scale: f64,
    margin_free: f64,
    ctx: &SizeCtx,
) -> LotOutcome {
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
    inst: &Instrument,
    mult: f64,
    px: f64,
    scale: f64,
    margin_free: f64,
    ctx: &SizeCtx,
) -> Option<Lot> {
    match make_lot_checked(sizing, fees, inst, mult, px, scale, margin_free, ctx) {
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

/// Close position `p` at the final fill price `px` (spread and slippage already in), record the
/// trade, settle cash, and return a Kelly summary.
#[allow(clippy::too_many_arguments)]
fn close_trade(
    trades: &mut Vec<Trade>,
    cash: &mut f64,
    fees: &Fees,
    mult: f64,
    b: &Bars,
    p: &Pos,
    px: f64,
    reason: &str,
    exit_bar: usize,
) -> ClosedTrade {
    let qty = p.qty();
    let avg = p.avg_price();
    let exit_fee = fee_for(fees, qty, px, mult);
    let gross = p.unrealized(px, mult);
    let entry_fees = p.entry_fees();
    let net = gross - entry_fees - exit_fee;
    // The entry fees left the cash at each fill, as a broker debits them: only the exit settles here.
    *cash += gross - exit_fee;
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
    ClosedTrade { ret: if notional != 0.0 { net / notional } else { 0.0 }, win: net > 0.0 }
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
    // Buy and hold over the *calendar window* the run was allowed to trade, not over the whole
    // dataset: a run restricted to 2021 is not measured against a benchmark that also banked
    // 2022. With no date rule the window is the whole series, which is the usual case.
    let (w0, w1) = s.filters.date_window(a0.ts).unwrap_or((0, a0.close.len().saturating_sub(1)));
    let bh = match (a0.close.get(w0).copied(), a0.close.get(w1).copied()) {
        (Some(first), Some(last)) if first > 0.0 => (last / first - 1.0) * 100.0,
        _ => 0.0,
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
fn limits_allow_open(
    s: &Settings,
    assets: &[Asset],
    ai: usize,
    px: f64,
    mult: f64,
    equity: f64,
    is_add: bool,
    at: Option<i64>,
) -> bool {
    let risk = &s.risk;
    if !is_add {
        if let Some(maxp) = risk.max_open_positions {
            // Held at this open counts, even when it closes later inside the bar: that slot is
            // not free yet when the entry is sent.
            if maxp > 0 && assets.iter().filter(|a| held_at(a, at)).count() >= maxp {
                return false;
            }
        }
    }
    // Exposure caps are re-checked against the *sized* lot in `exposure_allows_lot` once the
    // quantity is known. This pass only rejects a book that is already over the cap, which
    // saves sizing work; it can never be the whole check, because the lot about to fill is
    // exactly what pushes the book over.
    if let Some(cap) = risk.max_exposure_pct {
        if cap > 0.0 {
            let open: f64 = assets.iter().map(|a| open_notional(a, mult, at)).sum();
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
fn exposure_allows_lot(s: &Settings, assets: &[Asset], ai: usize, lot: &Lot, mult: f64, equity: f64, at: Option<i64>) -> bool {
    let risk = &s.risk;
    let add = lot.qty.abs() * lot.price.abs() * mult;
    if let Some(cap) = risk.max_exposure_pct {
        if cap > 0.0 {
            let open: f64 = assets.iter().map(|a| open_notional(a, mult, at)).sum();
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
        Bars { ticker: "", ts, open: px, high: px, low: px, close: px, volume: px, quotes: None, sub: None }
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
            execution: Execution::default(),
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
        Bars { ticker: "X", ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        Bars { ticker: "", ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        Bars { ticker: tk, ts, open: px, high: px, low: px, close: px, volume: px, quotes: None, sub: None }
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

    /// A length left at its default warms up like the default, and MACD's signal needs its slow
    /// average first.
    #[test]
    fn warmup_counts_defaults_and_chained_lengths() {
        assert_eq!(builtin_lookback("sma", 0, 0, 0, 0.0, 0), 20);
        assert_eq!(builtin_lookback("sma", 34, 0, 0, 0.0, 0), 34);
        assert!(builtin_lookback("macd_signal", 0, 0, 0, 0.0, 0) >= 26 + 9 - 1);
        assert!(builtin_lookback("macd_signal", 0, 12, 26, 0.0, 9) > 26);
        assert!(builtin_lookback("rsi", 0, 0, 0, 0.0, 0) >= 14);
    }

    // ── Phase 2: risk layer ──

    /// Instrument lot rounding + min size: raw qty rounds DOWN to lot_step; below min is skipped.
    #[test]
    fn instrument_lot_rounding_and_min() {
        let inst = Instrument { multiplier: 1.0, lot_step: 0.5, min_qty: 1.0, tick_size: 0.0 };
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
        s.instrument = Instrument { multiplier: 5.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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

    /// A named timezone follows daylight saving: 09:00 New York is 13:00 UTC in June and
    /// 14:00 UTC in January, and it wins over the fixed offset.
    #[test]
    fn session_filter_follows_daylight_saving() {
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.filters.sessions = vec![Session { from: "09:00".into(), to: "12:00".into() }];
        s.filters.timezone = "America/New_York".into();
        s.filters.tz_offset_min = 120;
        for (day, first) in [("2020-06-01", "2020-06-01T13:00:00Z"), ("2020-01-06", "2020-01-06T14:00:00Z")] {
            let (ts, px) = day_of_hours(day);
            let r = run(&s, &bars(&ts, &px));
            assert_eq!(r.trades[0].entry_ts, first);
            assert_eq!(r.filtered_bars, 21);
        }
        s.filters.timezone = "Mars/Olympus".into();
        assert!(s.filters.validate().is_some());
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

    /// A calendar window shortens the benchmark too: buy and hold is measured over the days the
    /// run was allowed to trade, not over the whole dataset.
    #[test]
    fn benchmark_follows_the_calendar_window() {
        let n = 6;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 20.0).collect(); // 100 → 200
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(0.0, 1.0)] });
        // Rows 1..=3 only: 120 → 160, i.e. +33.33%, against +100% over the full series.
        s.filters.include_dates =
            vec![DateRange { from: "2020-01-02".into(), to: Some("2020-01-04".into()) }];
        let r = run(&s, &bars(&ts, &c));
        approx(r.stats.buy_hold_return_pct, 100.0 / 3.0);
        approx(r.benchmark[0].equity, 10_000.0); // still in cash before the window opens
        approx(r.benchmark[1].equity, 10_000.0); // bought at 120
        approx(r.benchmark[3].equity, 10_000.0 * 160.0 / 120.0);
        approx(r.benchmark[n - 1].equity, 10_000.0 * 160.0 / 120.0); // frozen after it closes
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
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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
        s.instrument = Instrument { multiplier: 50.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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
        s.instrument = Instrument { multiplier: 10.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        Bars { ticker: tk, ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        s.instrument = Instrument { multiplier: 10.0, lot_step: 0.0, min_qty: 0.0, tick_size: 0.0 };
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

    /// Kelly: a breakeven trade counts in the window but is neither a win nor a loss. 10 wins of
    /// 0.10, 10 losses of 0.05 and 10 at zero: p = 1/3, b = 2, Kelly 0, so no entry.
    #[test]
    fn kelly_breakeven_is_not_a_win() {
        let closed: Vec<ClosedTrade> = (0..30)
            .map(|i| match i % 3 {
                0 => ClosedTrade { ret: 0.10, win: true },
                1 => ClosedTrade { ret: -0.05, win: false },
                _ => ClosedTrade { ret: 0.0, win: false },
            })
            .collect();
        let c = SizeCtx { equity: 10_000.0, entry_px: 100.0, stop_px: None, leverage: 1.0, mult: 1.0, closed: &closed };
        let q = resolve_qty(&Sizing::Kelly { fraction: 1.0, window: 30, cap_pct: 50.0, warmup: None }, &c);
        approx_v(q, 0.0, 1e-12, "breakevens must not raise the win rate");
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
        let inst = Instrument { multiplier: 1.0, lot_step: 0.5, min_qty: 0.0, tick_size: 0.0 };
        approx_v(inst.round_qty(1.7).unwrap(), 1.5, 1e-12, "1.7 ⇒ 1.5");
        approx_v(inst.round_qty(2.0).unwrap(), 2.0, 1e-12, "exact multiple kept");
        assert!(inst.round_qty(0.3).is_none(), "below one step ⇒ refused");
    }

    /// min_qty must refuse anything under it, even after rounding.
    #[test]
    fn v5_min_qty_refuses() {
        let inst = Instrument { multiplier: 1.0, lot_step: 0.1, min_qty: 1.0, tick_size: 0.0 };
        assert!(inst.round_qty(0.9).is_none(), "0.9 < min 1.0 ⇒ refused");
        assert!(inst.round_qty(1.05).is_some(), "1.0 after rounding ⇒ allowed");
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
        Bars { ticker: "X", ts, open: o, high: h, low: l, close: c, volume: c, quotes: None, sub: None }
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
        let b = Bars { ticker: "X", ts: &ts, open: &o, high: &h, low: &l, close: &c, volume: &c, quotes: None, sub: None };
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

/// Order types, the entry-bar test, the take profit as a limit, bid/ask pricing and the
/// lower-timeframe resolution of a bar where the stop and the target both print.
#[cfg(test)]
mod execution_tests {
    use super::tests::*;
    use super::*;

    fn ts(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("t{i}")).collect()
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    fn with_stops(sl: f64, tp: f64) -> Settings {
        let mut s = settings_always_long();
        let sd = s.long.as_mut().unwrap();
        sd.stop_loss_pct = sl;
        sd.take_profit_pct = tp;
        s
    }

    /// A trade opened at a bar's open is tested against that same bar: a candle that runs
    /// straight through the stop closes it there.
    #[test]
    fn stop_is_tested_on_the_entry_bar() {
        let t = ts(5);
        let o = [100.0; 5];
        let h = [100.5; 5];
        let l = [99.5, 90.0, 99.5, 99.5, 99.5];
        let c = [100.0; 5];
        let r = run(&with_stops(0.05, 0.0), &tests_bars(&t, &o, &h, &l, &c));
        let first = &r.trades[0];
        assert_eq!(first.exit_reason, "stop_loss");
        assert_eq!(first.exit_ts, "t1");
        assert_eq!(first.bars_held, 0);
        approx(first.exit_price, 95.0);
    }

    /// A position stopped inside a bar does not reopen at that bar's open, a price from before
    /// the stop: the always-true entry waits for the next bar.
    #[test]
    fn no_reentry_at_the_open_of_the_exit_bar() {
        let t = ts(5);
        let o = [100.0; 5];
        let h = [100.5; 5];
        let l = [99.5, 99.5, 90.0, 99.5, 99.5];
        let c = [100.0; 5];
        let r = run(&with_stops(0.05, 0.0), &tests_bars(&t, &o, &h, &l, &c));
        assert_eq!(r.trades[0].exit_reason, "stop_loss");
        assert_eq!(r.trades[0].exit_ts, "t2");
        assert_eq!(r.trades[1].entry_ts, "t3");
    }

    /// A bar that opens past the target fills the take profit at that open, before the stop the
    /// same bar later reaches.
    #[test]
    fn open_past_the_target_takes_profit_at_the_open() {
        let t = ts(4);
        let o = [100.0, 100.0, 105.0, 105.0];
        let h = [100.5, 100.5, 106.0, 106.0];
        let l = [99.5, 99.5, 90.0, 104.0];
        let c = [100.0, 100.0, 95.0, 105.0];
        let r = run(&with_stops(0.05, 0.03), &tests_bars(&t, &o, &h, &l, &c));
        let first = &r.trades[0];
        assert_eq!(first.exit_reason, "take_profit");
        assert_eq!(first.exit_ts, "t2");
        approx(first.exit_price, 105.0);
    }

    /// The take profit is a resting limit: it fills at the target, with no slippage and no half
    /// spread taken off it, once the bid reaches it.
    #[test]
    fn take_profit_fills_at_the_target() {
        let t = ts(4);
        let o = [100.0, 100.0, 101.0, 101.0];
        let h = [100.5, 100.5, 110.0, 101.5];
        let l = [99.5, 99.5, 100.5, 100.5];
        let c = [100.0, 100.0, 101.0, 101.0];
        let mut s = with_stops(0.0, 0.03);
        s.spread_pct = 0.002;
        s.slippage = Slippage { kind: "pct".into(), value: 0.001, tick_size: 0.0 };
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        let first = &r.trades[0];
        assert_eq!(first.exit_reason, "take_profit");
        approx(first.exit_price, first.entry_price * 1.03);
    }

    fn both_touched() -> (Vec<String>, [f64; 4], [f64; 4], [f64; 4], [f64; 4]) {
        // Entry at t1's open (100); t2 reaches both the stop (95) and the target (103).
        (ts(4), [100.0, 100.0, 100.0, 100.0], [101.5, 100.5, 104.0, 100.5], [99.5, 99.5, 94.0, 99.5], [101.0, 100.0, 100.0, 100.0])
    }

    /// One entry only: the signal holds on t0's close alone.
    fn once(mut s: Settings) -> Settings {
        s.long.as_mut().unwrap().entry = Some(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Price { field: "close".into() },
                op: Op::Above,
                right: Some(Operand::Const { value: 100.5 }),
            }],
        });
        s
    }

    /// An exit limit that expires unfilled is known to have expired only at its last close: its
    /// market order goes at the next open, which a new entry may then take as well.
    #[test]
    fn an_expired_exit_limit_leaves_at_the_next_open() {
        let t = ts(6);
        let o = [100.0, 100.0, 101.5, 101.5, 101.5, 101.5];
        let h = [100.7, 101.6, 101.8, 101.8, 101.8, 101.8];
        let l = [99.9, 99.9, 101.2, 101.2, 101.2, 101.2];
        let c = [100.6, 101.5, 101.5, 101.5, 101.5, 101.5];
        let mut s = settings_always_long();
        let sd = s.long.as_mut().unwrap();
        sd.entry = Some(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal { left: Operand::Price { field: "close".into() }, op: Op::Above, right: Some(Operand::Const { value: 100.5 }) }],
        });
        sd.exit = Some(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal { left: Operand::Price { field: "close".into() }, op: Op::Above, right: Some(Operand::Const { value: 101.0 }) }],
        });
        s.execution.exit_order = OrderSpec { kind: "limit".into(), offset: 0.01, ..OrderSpec::default() };
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        assert_eq!(r.trades[0].exit_ts, "t3");
        approx(r.trades[0].exit_price, 101.5);
        assert_eq!(r.trades[1].entry_ts, "t3", "{:?}", r.trades);
    }

    /// An ATR stop takes the ATR of the signal candle: the entry candle's own range is not known
    /// at its open. Measured on the entry candle (21 wide), the stop would sit under its low.
    #[test]
    fn atr_stop_uses_the_signal_candle() {
        let t = ts(6);
        let o = [100.0; 6];
        let h = [101.0; 6];
        let l = [99.0, 99.0, 99.0, 80.0, 99.0, 99.0];
        let c = [100.0, 100.0, 100.6, 100.0, 100.0, 100.0];
        let mut s = once(settings_always_long());
        s.long.as_mut().unwrap().stop_loss = Some(Stop { kind: "atr".into(), value: 1.0, period: 1 });
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        let first = &r.trades[0];
        assert_eq!(first.entry_ts, "t3");
        assert_eq!(first.exit_reason, "stop_loss");
        assert_eq!(first.exit_ts, "t3");
        approx(first.exit_price, 98.0);
    }

    /// The bar alone cannot order the stop and the target: the stop wins, and the run says so.
    #[test]
    fn stop_wins_an_unresolved_tie() {
        let (t, o, h, l, c) = both_touched();
        let r = run(&once(with_stops(0.05, 0.03)), &tests_bars(&t, &o, &h, &l, &c));
        assert_eq!(r.trades[0].exit_reason, "stop_loss");
        assert_eq!(r.execution.ambiguous_bars, 1);
        assert_eq!(r.execution.unresolved_bars, 1);
    }

    /// A fixed set of candles under one bar, standing in for the API's loader.
    struct Fixed {
        bar: usize,
        high: Vec<f64>,
        asked: std::sync::Mutex<Vec<usize>>,
    }
    impl SubSource for Fixed {
        fn candles(&self, bar: usize) -> Option<std::sync::Arc<SubSlice>> {
            self.asked.lock().unwrap().push(bar);
            (bar == self.bar).then(|| {
                std::sync::Arc::new(SubSlice {
                    open: vec![100.0, 103.5, 99.0],
                    high: self.high.clone(),
                    low: vec![99.8, 99.0, 94.0],
                    quotes: None,
                })
            })
        }
        fn timeframe(&self) -> Option<String> {
            Some("1m".into())
        }
    }

    /// The lower timeframe orders them: here the target prints in the first sub-bar. The source
    /// is asked for that bar only.
    #[test]
    fn lower_timeframe_settles_the_tie() {
        let (t, o, h, l, c) = both_touched();
        let src = Fixed { bar: 2, high: vec![104.0, 103.9, 99.5], asked: Default::default() };
        let mut b = tests_bars(&t, &o, &h, &l, &c);
        b.sub = Some(&src);
        let mut s = once(with_stops(0.05, 0.03));
        s.execution.intrabar = true;
        let r = run(&s, &b);
        assert_eq!(r.trades[0].exit_reason, "take_profit");
        approx(r.trades[0].exit_price, 103.0);
        assert_eq!(r.execution.resolved_bars, 1);
        assert_eq!(r.execution.sub_timeframe.as_deref(), Some("1m"));
        assert_eq!(*src.asked.lock().unwrap(), vec![2]);

        // Sub-bars that disagree with the bar are not used: back to the worst case.
        let bad = Fixed { bar: 2, high: vec![102.0, 101.0, 99.5], asked: Default::default() };
        b.sub = Some(&bad);
        let r = run(&s, &b);
        assert_eq!(r.trades[0].exit_reason, "stop_loss");
        assert_eq!(r.execution.unresolved_bars, 1);
    }

    /// A limit entry rests below the signal close and fills at its own price, no slippage.
    #[test]
    fn limit_entry_fills_at_its_price() {
        let t = ts(4);
        let o = [100.0; 4];
        let h = [100.5; 4];
        let l = [99.5, 98.5, 99.5, 99.5];
        let c = [100.0; 4];
        let mut s = settings_always_long();
        s.slippage = Slippage { kind: "pct".into(), value: 0.001, tick_size: 0.0 };
        s.execution.entry_order = OrderSpec { kind: "limit".into(), offset: 0.01, ..OrderSpec::default() };
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        assert_eq!(r.trades[0].entry_ts, "t1");
        approx(r.trades[0].entry_price, 99.0);
        assert!(r.execution.orders_filled >= 1);
    }

    /// An entry limit the market never reaches expires after its validity.
    #[test]
    fn limit_entry_expires() {
        let t = ts(5);
        let o = [100.0; 5];
        let h = [100.5; 5];
        let l = [99.5; 5];
        let c = [100.0; 5];
        let mut s = settings_always_long();
        s.execution.entry_order = OrderSpec { kind: "limit".into(), offset: 0.02, valid_bars: 2, ..OrderSpec::default() };
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        assert!(r.trades.is_empty());
        assert!(r.execution.orders_expired >= 1);
        assert_eq!(r.execution.orders_filled, 0);
    }

    /// An exit limit that does not fill within its validity goes to market at the open after its
    /// last bar, and the order is counted as expired.
    #[test]
    fn exit_limit_expires_to_market() {
        let t = ts(5);
        let o = [100.0; 5];
        let h = [100.5; 5];
        let l = [99.5; 5];
        let c = [100.0, 100.0, 100.2, 100.0, 100.0];
        let mut s = settings_always_long();
        s.long.as_mut().unwrap().exit = Some(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.execution.exit_order = OrderSpec { kind: "limit".into(), offset: 0.05, ..OrderSpec::default() };
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        let first = &r.trades[0];
        assert_eq!(first.exit_reason, "exit_signal");
        assert_eq!(first.exit_ts, "t3");
        approx(first.exit_price, 100.0);
        assert!(r.execution.orders_expired >= 1);
    }

    /// With bid/ask stored, a market buy pays the ask and the spread setting is not added on top.
    #[test]
    fn quotes_price_a_market_entry_at_the_ask() {
        let t = ts(4);
        let o = [100.0; 4];
        let h = [100.5; 4];
        let l = [99.5; 4];
        let c = [100.0; 4];
        let q = Quote {
            bid_open: 99.7,
            bid_high: 100.2,
            bid_low: 99.2,
            bid_close: 99.7,
            ask_open: 100.3,
            ask_high: 100.8,
            ask_low: 99.8,
            ask_close: 100.3,
        };
        let quotes = vec![Some(q); 4];
        let mut b = tests_bars(&t, &o, &h, &l, &c);
        b.quotes = Some(&quotes);
        let mut s = settings_always_long();
        s.spread_pct = 0.01;
        s.execution.use_quotes = true;
        let r = run(&s, &b);
        approx(r.trades[0].entry_price, 100.3);
        // Closed by "end" at the last close, on the bid.
        approx(r.trades[0].exit_price, 99.7);
        assert_eq!(r.execution.quoted_fills, r.execution.fills);
    }

    /// A fill the live price took replaces the candle's verdict on its bar.
    #[test]
    fn live_exit_is_replayed_as_given() {
        let t = ts(5);
        let o = [100.0; 5];
        let h = [100.5; 5];
        let l = [99.5; 5];
        let c = [101.0, 100.0, 100.0, 100.0, 100.0];
        let s = once_long(with_stops(0.05, 0.0));
        let forced = vec![vec![ForcedFill { bar: 3, kind: ForcedKind::Exit { px: 97.0, reason: "stop_loss".into(), maker: false } }]];
        let r = run_portfolio_forced(&s, &[&tests_bars(&t, &o, &h, &l, &c)], &forced);
        assert_eq!(r.trades[0].exit_reason, "stop_loss");
        assert_eq!(r.trades[0].exit_ts, "t3");
        approx(r.trades[0].exit_price, 97.0);
        assert_eq!(r.execution.forced_unmatched, 0);

        // A live fill the replay has no position for is counted, never applied.
        let stray = vec![vec![ForcedFill { bar: 0, kind: ForcedKind::Exit { px: 97.0, reason: "stop_loss".into(), maker: false } }]];
        let r = run_portfolio_forced(&s, &[&tests_bars(&t, &o, &h, &l, &c)], &stray);
        assert_eq!(r.execution.forced_unmatched, 1);
    }

    /// A position still open after the last bar exports the levels it holds for the next one,
    /// the trailing stop ratcheted on the bar that just closed.
    #[test]
    fn open_position_exports_its_levels() {
        let t = ts(4);
        let o = [100.0, 100.0, 101.0, 104.0];
        let h = [100.5, 100.5, 102.0, 106.0];
        let l = [99.5, 99.5, 100.5, 103.5];
        let c = [101.0, 100.0, 101.5, 105.0];
        let mut s = once_long(with_stops(0.05, 0.10));
        s.long.as_mut().unwrap().trailing_stop = Some(Trail { kind: "pct".into(), value: 0.02, ..Trail::default() });
        let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
        let p = &r.open_positions[0];
        approx(p.avg_price, 100.0);
        approx(p.take_profit.unwrap(), 110.0);
        // Trail 2% under the last bar's 106 high is tighter than the 95 stop.
        approx(p.stop.unwrap(), 106.0 * 0.98);
        assert_eq!(p.stop_reason, "trailing_stop");
    }

    /// Bid/ask is asked for the bars where a fill can happen, not for every bar held.
    #[test]
    fn quotes_are_read_only_where_a_fill_can_happen() {
        struct Count(std::sync::Mutex<Vec<usize>>);
        impl QuoteSource for Count {
            fn quote(&self, bar: usize) -> Option<Quote> {
                self.0.lock().unwrap().push(bar);
                None
            }
        }
        let n = 9;
        let t = ts(n);
        let flat = vec![100.0; n];
        let h = vec![100.2; n];
        let l = vec![99.8; n];
        let mut c = flat.clone();
        c[0] = 101.0;
        let src = Count(Default::default());
        let mut b = tests_bars(&t, &flat, &h, &l, &c);
        b.quotes = Some(&src);
        let mut s = once_long(with_stops(0.05, 0.10));
        s.execution.use_quotes = true;
        run(&s, &b);
        // The entry bar and the last close; the levels are far from every bar in between.
        assert_eq!(*src.0.lock().unwrap(), vec![1, n - 1]);
    }

    /// One long entry: the signal holds on t0's close alone (all closes equal, t0 above).
    fn once_long(mut s: Settings) -> Settings {
        s.long.as_mut().unwrap().entry = Some(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Price { field: "close".into() },
                op: Op::Above,
                right: Some(Operand::Const { value: 100.5 }),
            }],
        });
        s.long.as_mut().unwrap().exit = None;
        s
    }

    #[test]
    fn execution_settings_are_validated() {
        let mut s = settings_always_long();
        s.execution.entry_order.kind = "stop".into();
        assert!(s.validate().is_some());
        let mut s = settings_always_long();
        s.execution.exit_order = OrderSpec { kind: "limit".into(), on_expiry: "never".into(), ..OrderSpec::default() };
        assert!(s.validate().is_some());
        let mut s = settings_always_long();
        s.execution.intrabar_timeframe = "2m".into();
        assert!(s.validate().is_some());
        assert!(settings_always_long().validate().is_none());
    }
}

/// Fixes from the external validation framework (`scripts/backtest-validation/framework`).
#[cfg(test)]
mod framework_fixes {
    use super::tests::{settings_always_long, tests_bars};
    use super::*;

    fn days(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("{}T00:00:00Z", time::Date::from_ordinal_date(2024, 1).unwrap() + time::Duration::days(i as i64)))
            .collect()
    }

    #[test]
    fn the_entry_fee_leaves_the_cash_at_the_fill() {
        let ts = days(5);
        let px = vec![100.0; 5];
        let b = tests_bars(&ts, &px, &px, &px, &px);
        let mut s = settings_always_long();
        s.sizing = Sizing::FixedQty { qty: 10.0 };
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 1.0 };
        let r = run(&s, &b);
        // Bar 1 opens the position: its 10.00 fee is gone from the equity that same bar.
        assert!((r.equity[1].equity - 9_990.0).abs() < 1e-9, "{}", r.equity[1].equity);
        let t = &r.trades[0];
        assert!((t.fees - 20.0).abs() < 1e-9 && (t.pnl + 20.0).abs() < 1e-9);
        assert!((r.stats.final_equity - 9_980.0).abs() < 1e-9);
    }

    #[test]
    fn fills_and_levels_sit_on_the_tick_against_the_trader() {
        let ts = days(5);
        let o = [100.0, 100.0, 100.0, 100.0, 100.0];
        let h = [100.0, 100.0, 100.0, 100.0, 100.0];
        let l = [100.0, 100.0, 100.0, 98.1, 100.0];
        let b = tests_bars(&ts, &o, &h, &l, &o);
        let mut s = settings_always_long();
        s.spread_pct = 0.001;
        s.instrument.tick_size = 0.25;
        s.long.as_mut().unwrap().stop_loss_pct = 0.02;
        let r = run(&s, &b);
        let t = &r.trades[0];
        // 100 × 1.0005 = 100.05 → the buy pays the tick above.
        assert_eq!(t.entry_price, 100.25);
        // Stop 2% under 100.25 = 98.245 → placed on 98.00, which a 98.10 low does not reach.
        assert_eq!(t.exit_reason, "end");
        // 100 × 0.9995 = 99.95 → the sell gets the tick below.
        assert_eq!(t.exit_price, 99.75);
        assert_eq!(on_tick(98.245, 0.25, false), 98.0);
        assert_eq!(on_tick(100.0, 0.01, true), 100.0);
        assert_eq!(on_tick(0.30000000000000004, 0.1, true), 0.30000000000000004);
    }

    #[test]
    fn a_neutral_grid_trades_both_sides_of_its_centre() {
        let p = [100.0, 89.0, 100.0, 111.0, 100.0, 89.0, 100.0];
        let ts = days(p.len());
        let b = tests_bars(&ts, &p, &p, &p, &p);
        let mut s = settings_always_long();
        s.kind = "grid".into();
        s.grid = Some(GridConfig { lower: 80.0, upper: 120.0, levels: 5, qty_per_level: 1.0, direction: "neutral".into(), ..GridConfig::default() });
        assert!(s.validate().is_none());
        let r = run_portfolio(&s, &[&b]);
        let dirs: std::collections::BTreeSet<&str> = r.trades.iter().map(|t| t.direction.as_str()).collect();
        assert_eq!(dirs, ["long", "short"].into_iter().collect(), "{:?}", r.trades.iter().map(|t| (&t.direction, t.entry_price, t.exit_price)).collect::<Vec<_>>());
        for t in &r.trades {
            if t.direction == "long" {
                assert!(t.entry_price < 100.0 + 1e-9 && t.exit_price <= 100.0 + 1e-9);
            } else {
                assert!(t.entry_price >= 110.0 - 1e-9 && t.exit_price >= 100.0 - 1e-9);
            }
        }
        // An even number of levels has no centre line.
        s.grid.as_mut().unwrap().levels = 6;
        assert!(s.validate().unwrap().contains("odd"));
    }

    #[test]
    fn kelly_resumes_once_the_theoretical_record_turns_positive() {
        // 40 bars falling 1%/bar (every trade stopped out), then 80 rising 1%/bar (every trade
        // takes its target): the first window is all losses, so Kelly skips; the skipped trades
        // still count, and once they win the strategy trades again.
        let mut px = vec![100.0];
        for i in 1..120 {
            let k = if i < 40 { 0.99 } else { 1.01 };
            px.push(px[i - 1] * k);
        }
        let ts = days(px.len());
        let (h, l): (Vec<f64>, Vec<f64>) = px
            .iter()
            .enumerate()
            .map(|(i, p)| if i < 40 { (p * 1.001, p * 0.98) } else { (p * 1.02, p * 0.999) })
            .unzip();
        let b = tests_bars(&ts, &px, &h, &l, &px);
        let mut s = settings_always_long();
        s.long.as_mut().unwrap().stop_loss_pct = 0.005;
        s.long.as_mut().unwrap().take_profit_pct = 0.005;
        s.sizing = Sizing::Kelly { fraction: 0.5, window: 5, cap_pct: 20.0, warmup: Some(Box::new(Sizing::FixedQty { qty: 1.0 })) };
        let r = run(&s, &b);
        let late = r.trades.iter().filter(|t| t.entry_ts > ts[60]).count();
        assert!(late > 10, "Kelly never resumed: {} trades after the turn", late);
    }

    #[test]
    fn meaningless_numbers_are_refused_and_fees_may_be_negative() {
        let ok = settings_always_long();
        assert!(ok.validate().is_none());
        let refuse = |f: &dyn Fn(&mut Settings)| {
            let mut s = settings_always_long();
            f(&mut s);
            s.validate().is_some()
        };
        assert!(refuse(&|s| s.starting_capital = 0.0));
        assert!(refuse(&|s| s.starting_capital = -1.0));
        assert!(refuse(&|s| s.leverage = 0.0));
        assert!(refuse(&|s| s.sizing = Sizing::FixedQty { qty: -1.0 }));
        assert!(refuse(&|s| s.instrument.multiplier = -5.0));
        assert!(refuse(&|s| s.instrument.lot_step = -1.0));
        assert!(refuse(&|s| s.instrument.tick_size = -0.01));
        assert!(refuse(&|s| s.slippage = Slippage { kind: "ticks".into(), value: 2.0, tick_size: 0.0 }));
        assert!(!refuse(&|s| {
            s.slippage = Slippage { kind: "ticks".into(), value: 2.0, tick_size: 0.0 };
            s.instrument.tick_size = 0.5;
        }));
        assert!(!refuse(&|s| s.fees.amount = -0.02));
        assert!(!refuse(&|s| s.execution.maker_fee = Some(-0.01)));
        let grid = |g: GridConfig| {
            let mut s = settings_always_long();
            s.kind = "grid".into();
            s.grid = Some(g);
            s.validate()
        };
        let band = |lo: f64, hi: f64, levels: usize| GridConfig { lower: lo, upper: hi, levels, qty_per_level: 1.0, ..GridConfig::default() };
        assert!(grid(band(100.0, 100.0, 5)).is_some());
        assert!(grid(band(110.0, 90.0, 5)).is_some());
        assert!(grid(band(90.0, 110.0, 1)).is_some());
        assert!(grid(band(90.0, 110.0, 201)).is_some());
        assert!(grid(band(0.0, 0.0, 5)).is_none(), "known-range ladder");
        assert!(grid(GridConfig { total_budget: -1.0, ..band(90.0, 110.0, 5) }).is_some());
        let mut dca = settings_always_long();
        dca.kind = "dca".into();
        dca.dca = Some(dca::DcaConfig::default());
        dca.starting_capital = 0.0;
        assert!(dca.validate().is_none(), "a plan may start from nothing");
    }
}
