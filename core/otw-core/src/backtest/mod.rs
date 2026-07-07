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

mod indicators;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
}

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

/// How to size each entry (each pyramiding add is sized independently by the same rule).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Sizing {
    /// Percent of current equity put into the position notional.
    PercentEquity { percent: f64 },
    /// Fixed quantity / lots / contracts (units of the instrument).
    FixedQty { qty: f64 },
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
    /// Exit (at close) when this side's entry group no longer holds.
    #[serde(default)]
    pub exit_on_reverse: bool,
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

/// Full run configuration. Mirrors the settings JSON the frontend posts and stores.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
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
}

/// OHLCV input (parallel arrays mirror the histdata bars endpoint).
pub struct Bars<'a> {
    pub ts: &'a [String],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub close: &'a [f64],
    pub volume: &'a [f64],
}

#[derive(Debug, Serialize)]
pub struct Trade {
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

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub trades: Vec<Trade>,
    pub equity: Vec<EquityPoint>,
    pub stats: Stats,
}

/// Resolve an operand to a per-bar series (None where undefined for that bar).
fn resolve(op: &Operand, b: &Bars) -> Vec<Option<f64>> {
    match op {
        Operand::Const { value } => vec![Some(*value); b.close.len()],
        Operand::Price { field } => {
            let src = match field.as_str() {
                "open" => b.open,
                "high" => b.high,
                "low" => b.low,
                "volume" => b.volume,
                _ => b.close,
            };
            src.iter().map(|&v| Some(v)).collect()
        }
        Operand::Indicator { indicator, period, fast, slow, mult, signal_period } => {
            use indicators as ind;
            let p = |d: usize| if *period == 0 { d } else { *period };
            let f = |d: usize| if *fast == 0 { d } else { *fast };
            let sl = |d: usize| if *slow == 0 { d } else { *slow };
            let m = |d: f64| if *mult <= 0.0 { d } else { *mult };
            let sp = |d: usize| if *signal_period == 0 { d } else { *signal_period };
            match indicator.as_str() {
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
    }
}

/// Evaluate one signal into a per-bar boolean (true = condition holds at bar i).
fn eval_signal(s: &Signal, b: &Bars) -> Vec<bool> {
    let n = b.close.len();
    let left = resolve(&s.left, b);
    let right = s.right.as_ref().map(|r| resolve(r, b));
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
fn eval_group(g: &SignalGroup, b: &Bars) -> Vec<bool> {
    let n = b.close.len();
    if g.conditions.is_empty() {
        return vec![false; n];
    }
    let per: Vec<Vec<bool>> = g.conditions.iter().map(|s| eval_signal(s, b)).collect();
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

fn fee_for(f: &Fees, qty: f64, price: f64) -> f64 {
    let notional = qty.abs() * price.abs();
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

/// Live position state during the simulation.
struct Pos {
    long: bool,
    lots: Vec<Lot>,
    entry_idx: usize,
    /// Bar index of the latest add — guards against double-adding on one bar.
    last_add_idx: usize,
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
    /// Unrealized PnL (gross) at `px`.
    fn unrealized(&self, px: f64) -> f64 {
        let d = if self.long { 1.0 } else { -1.0 };
        self.lots.iter().map(|l| d * l.qty * (px - l.price)).sum()
    }
}

/// Per-side precomputed signals for the run loop.
struct SideSignals {
    entry: Option<Vec<bool>>,
    exit: Option<Vec<bool>>,
    sl: f64,
    tp: f64,
    exit_on_reverse: bool,
}

fn side_signals(side: Option<&Side>, enabled: bool, b: &Bars) -> SideSignals {
    let side = side.filter(|_| enabled);
    SideSignals {
        entry: side.and_then(|sd| sd.entry_group()).map(|g| eval_group(&g, b)),
        exit: side
            .and_then(|sd| sd.exit.as_ref())
            .filter(|g| !g.conditions.is_empty())
            .map(|g| eval_group(g, b)),
        sl: side.map(|x| x.stop_loss_pct).unwrap_or(0.0),
        tp: side.map(|x| x.take_profit_pct).unwrap_or(0.0),
        exit_on_reverse: side.map(|x| x.exit_on_reverse).unwrap_or(false),
    }
}

/// Run the backtest. One position (direction) at a time; each side fires from its own entry
/// group. Entries fill at the *next* bar open after the group fires (no lookahead), and
/// re-fires stack up to `pyramiding` entries. Exit priority within a bar: explicit exit
/// group (at open) → stop-and-reverse (at open) → SL → TP (intrabar, stop first —
/// conservative) → entry-group-reversal (at close) → dataset end (at close). SL/TP are
/// percent from the volume-weighted average entry, so they re-anchor when pyramiding adds.
pub fn run(s: &Settings, b: &Bars) -> RunResult {
    let n = b.close.len();
    let half_spread = s.spread_pct / 2.0;
    let pyramiding = s.pyramiding.clamp(1, 20);

    let long = side_signals(s.long.as_ref(), s.allow_long(), b);
    let short = side_signals(s.short.as_ref(), s.allow_short(), b);

    let mut trades: Vec<Trade> = Vec::new();
    let mut equity = Vec::with_capacity(n);
    let mut cash = s.starting_capital;
    let mut peak = cash;
    let mut max_dd = 0.0_f64;
    let mut max_dd_abs = 0.0_f64;
    let mut pos: Option<Pos> = None;

    // Close `p` at `raw_px`, recording the trade and settling cash.
    let close_pos = |trades: &mut Vec<Trade>, cash: &mut f64, p: &Pos, raw_px: f64, reason: &str, exit_idx: usize| {
        let px = if p.long { raw_px * (1.0 - half_spread) } else { raw_px * (1.0 + half_spread) };
        let qty = p.qty();
        let avg = p.avg_price();
        let exit_fee = fee_for(&s.fees, qty, px);
        let gross = p.unrealized(px);
        let entry_fees = p.entry_fees();
        let net = gross - entry_fees - exit_fee;
        *cash += net;
        let ret = if avg * qty != 0.0 { net / (avg * qty.abs()) * 100.0 } else { 0.0 };
        trades.push(Trade {
            entry_ts: b.ts[p.entry_idx].clone(),
            exit_ts: b.ts[exit_idx].clone(),
            entry_price: avg,
            exit_price: px,
            qty,
            entries: p.lots.len(),
            direction: if p.long { "long".into() } else { "short".into() },
            exit_reason: reason.into(),
            pnl: net,
            fees: entry_fees + exit_fee,
            return_pct: ret,
            bars_held: exit_idx.saturating_sub(p.entry_idx),
        });
    };

    // Build one sized lot at this bar's open (or None if size 0).
    let make_lot = |cash: f64, long: bool, i: usize| -> Option<Lot> {
        let raw = b.open[i];
        let px = if long { raw * (1.0 + half_spread) } else { raw * (1.0 - half_spread) };
        let q = match &s.sizing {
            Sizing::PercentEquity { percent } => {
                let notional = cash * (percent / 100.0) * s.leverage;
                if px > 0.0 { notional / px } else { 0.0 }
            }
            Sizing::FixedQty { qty } => *qty,
        };
        if q <= 0.0 {
            return None;
        }
        Some(Lot { price: px, qty: q, fee: fee_for(&s.fees, q, px) })
    };

    let fired = |sig: &Option<Vec<bool>>, i: usize| i > 0 && sig.as_ref().is_some_and(|v| v[i - 1]);
    let holds = |sig: &Option<Vec<bool>>, i: usize| sig.as_ref().is_some_and(|v| v[i]);

    for i in 0..n {
        // ── Manage open position ──
        if let Some(p) = &pos {
            let own = if p.long { &long } else { &short };
            let avg = p.avg_price();
            // Opposite entry firing this bar (for stop-and-reverse).
            let opp_fires = if p.long { fired(&short.entry, i) } else { fired(&long.entry, i) };

            let mut exit = None; // (price, reason, reverse?)
            if fired(&own.exit, i) {
                exit = Some((b.open[i], "exit_signal", false));
            }
            if exit.is_none() && s.stop_and_reverse && opp_fires {
                exit = Some((b.open[i], "reverse", true));
            }
            if exit.is_none() && own.sl > 0.0 {
                let stop = if p.long { avg * (1.0 - own.sl) } else { avg * (1.0 + own.sl) };
                if if p.long { b.low[i] <= stop } else { b.high[i] >= stop } {
                    exit = Some((stop, "stop_loss", false));
                }
            }
            if exit.is_none() && own.tp > 0.0 {
                let target = if p.long { avg * (1.0 + own.tp) } else { avg * (1.0 - own.tp) };
                if if p.long { b.high[i] >= target } else { b.low[i] <= target } {
                    exit = Some((target, "take_profit", false));
                }
            }
            if exit.is_none() && own.exit_on_reverse && !holds(&own.entry, i) {
                exit = Some((b.close[i], "signal", false));
            }
            if exit.is_none() && i == n - 1 {
                exit = Some((b.close[i], "end", false));
            }

            if let Some((raw_px, reason, reverse)) = exit {
                let was_long = p.long;
                let p = pos.take().unwrap();
                close_pos(&mut trades, &mut cash, &p, raw_px, reason, i);
                // Stop-and-reverse: open the opposite side immediately at this bar's open.
                if reverse && i != n - 1 {
                    pos = make_lot(cash, !was_long, i).map(|lot| Pos {
                        long: !was_long,
                        lots: vec![lot],
                        entry_idx: i,
                        last_add_idx: i,
                    });
                }
            }
        }

        // ── Open on a side's entry group (fills at this bar's open if it fired on i-1),
        //    or stack a pyramiding add onto the open position. ──
        if i != n - 1 {
            match &mut pos {
                None => {
                    if fired(&long.entry, i) {
                        pos = make_lot(cash, true, i)
                            .map(|lot| Pos { long: true, lots: vec![lot], entry_idx: i, last_add_idx: i });
                    } else if fired(&short.entry, i) {
                        pos = make_lot(cash, false, i)
                            .map(|lot| Pos { long: false, lots: vec![lot], entry_idx: i, last_add_idx: i });
                    }
                }
                Some(p) => {
                    let own_fires = if p.long { fired(&long.entry, i) } else { fired(&short.entry, i) };
                    if own_fires && p.lots.len() < pyramiding && p.last_add_idx != i {
                        if let Some(lot) = make_lot(cash, p.long, i) {
                            p.lots.push(lot);
                            p.last_add_idx = i;
                        }
                    }
                }
            }
        }

        // Mark-to-market equity at close.
        let eq = match &pos {
            None => cash,
            Some(p) => cash + p.unrealized(b.close[i]),
        };
        peak = peak.max(eq);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - eq) / peak * 100.0);
        }
        max_dd_abs = max_dd_abs.max(peak - eq);
        equity.push(EquityPoint { ts: b.ts[i].clone(), equity: eq });
    }

    // ── Stats ──
    let all = side_stats(trades.iter());
    let long_s = side_stats(trades.iter().filter(|t| t.direction == "long"));
    let short_s = side_stats(trades.iter().filter(|t| t.direction == "short"));
    let mut exit_reasons = BTreeMap::new();
    for t in &trades {
        *exit_reasons.entry(t.exit_reason.clone()).or_insert(0) += 1;
    }
    let (sharpe, sortino) = risk_ratios(&equity);

    let net_pnl = cash - s.starting_capital;
    let stats = Stats {
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
        buy_hold_return_pct: if b.close[0] > 0.0 {
            (b.close[n - 1] / b.close[0] - 1.0) * 100.0
        } else {
            0.0
        },
        sharpe,
        sortino,
        expectancy_pct: all.expectancy_pct,
        exit_reasons,
        all,
        long: long_s,
        short: short_s,
    };

    RunResult { trades, equity, stats }
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
        Bars { ts, open: px, high: px, low: px, close: px, volume: px }
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
            mode: Mode::Long,
            long: Some(Side {
                signal: None,
                entry: Some(entry),
                exit: None,
                stop_loss_pct: 0.0,
                take_profit_pct: 0.0,
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
}
