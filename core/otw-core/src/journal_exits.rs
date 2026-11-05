//! Exit-signal lab for the Trading Journal.
//!
//! The trader picks closed trades whose candles are already in store, and the lab replays
//! each of them under a catalog of simple exit rules (a fixed stop, an ATR target, an RSI
//! reading, a moving-average cross…). Every rule keeps the trader's own entry, size and
//! fees: only the exit moves, so the difference against the real result is the price of
//! the exit and nothing else.
//!
//! Two semantics, chosen per run:
//!
//! * **overlay** (default): the rule is added to what the trader did. It can only close a
//!   trade *before* the real exit; a rule that does not fire by then leaves the trade as it
//!   was. This needs no candle after the real exit, so it works on what the enrichment
//!   already downloaded.
//! * **replace**: the rule *is* the exit. It may hold past the real exit, up to
//!   `extend_bars` candles, when those candles are in store. A rule that never fires keeps
//!   the real exit rather than inventing one.
//!
//! A stop never extends a trade, so a stop-loss rule is an overlay in both modes, except
//! inside a combination with a target that already extended it.
//!
//! Fills are deliberately plain: a level (stop, target) fills at the level, or at the open
//! when the bar gapped through it; an indicator rule is read on the close and fills there.
//! When a stop and a target sit inside the same bar, the stop is assumed first. Nothing
//! reads a value its bar had not printed yet: a band or a channel used as an intrabar level
//! is the previous bar's.
//!
//! Never a guess: a trade whose candles are not in store, whose FX cannot be priced or
//! which is still open is reported as skipped with the reason, and a rule that cannot apply
//! to a trade (an R target on a trade with no planned stop) says so instead of firing.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata as store;
use otw_store::journal::{self, Trade, TradeFilter};
use otw_store::journal_analytics::{trade_risk, trade_stop};
use otw_store::journal_market as market_store;

use crate::histdata;
use crate::journal_market::{self, pick_connector, provider_symbol, trade_window};

/// Candles read before the entry, so a 26/9 MACD or a 30-bar average is seeded when the
/// store holds them. Fewer is not an error: a rule simply cannot fire until its indicator
/// exists, and `warmup_bars` on the trade says how thin the history was.
const WARMUP_BARS: i64 = 100;
/// Hard cap on candles pulled for one trade, same reason as the enrichment's.
const MAX_BARS_PER_TRADE: i64 = 20_000;
/// Trades one run accepts. The lab is a comparison screen, not a batch job.
pub const MAX_TRADES: usize = 500;
/// Signals one run accepts (the agent passes its own variants).
pub const MAX_SIGNALS: usize = 60;
/// Default and ceiling of the post-exit window in `replace` mode.
const DEFAULT_EXTEND: usize = 20;
const MAX_EXTEND: usize = 500;

// ── Catalog ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Protective stop: a price level against the position.
    Sl,
    /// Take-profit: a level or reading in favour of the position.
    Tp,
    /// Exit read on the close (trend change, momentum fade, time).
    Exit,
}

/// One tunable parameter of a rule, with the range the agent is allowed to search.
#[derive(Debug, Serialize)]
pub struct ParamSpec {
    pub key: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Integer parameters (periods, bar counts) are rounded before use.
    pub integer: bool,
}

#[derive(Debug, Serialize)]
pub struct RuleSpec {
    pub rule: &'static str,
    pub role: Role,
    pub params: &'static [ParamSpec],
}

const fn p(key: &'static str, min: f64, max: f64, step: f64, integer: bool) -> ParamSpec {
    ParamSpec { key, min, max, step, integer }
}

/// Every rule the lab knows. A preset below is one of these with its parameters filled.
pub const RULES: &[RuleSpec] = &[
    // Stops.
    RuleSpec { rule: "sl_pct", role: Role::Sl, params: &[p("pct", 0.1, 50.0, 0.1, false)] },
    RuleSpec { rule: "sl_atr", role: Role::Sl, params: &[p("mult", 0.25, 10.0, 0.25, false), p("period", 2.0, 100.0, 1.0, true)] },
    RuleSpec { rule: "sl_trail_pct", role: Role::Sl, params: &[p("pct", 0.1, 50.0, 0.1, false)] },
    RuleSpec { rule: "sl_trail_atr", role: Role::Sl, params: &[p("mult", 0.25, 10.0, 0.25, false), p("period", 2.0, 100.0, 1.0, true)] },
    RuleSpec { rule: "sl_donchian", role: Role::Sl, params: &[p("n", 2.0, 200.0, 1.0, true)] },
    RuleSpec { rule: "sl_swing", role: Role::Sl, params: &[p("n", 2.0, 200.0, 1.0, true)] },
    RuleSpec { rule: "sl_planned", role: Role::Sl, params: &[] },
    // Targets.
    RuleSpec { rule: "tp_pct", role: Role::Tp, params: &[p("pct", 0.1, 200.0, 0.1, false)] },
    RuleSpec { rule: "tp_atr", role: Role::Tp, params: &[p("mult", 0.25, 20.0, 0.25, false), p("period", 2.0, 100.0, 1.0, true)] },
    RuleSpec { rule: "tp_r", role: Role::Tp, params: &[p("r", 0.25, 20.0, 0.25, false)] },
    RuleSpec { rule: "tp_rsi", role: Role::Tp, params: &[p("period", 2.0, 100.0, 1.0, true), p("level", 50.0, 99.0, 1.0, false)] },
    RuleSpec { rule: "tp_bb", role: Role::Tp, params: &[p("period", 5.0, 200.0, 1.0, true), p("k", 0.5, 5.0, 0.1, false)] },
    RuleSpec { rule: "tp_donchian", role: Role::Tp, params: &[p("n", 2.0, 200.0, 1.0, true)] },
    // Close-read exits.
    RuleSpec { rule: "x_ma_cross", role: Role::Exit, params: &[p("fast", 2.0, 200.0, 1.0, true), p("slow", 3.0, 400.0, 1.0, true), p("ema", 0.0, 1.0, 1.0, true)] },
    RuleSpec { rule: "x_close_ma", role: Role::Exit, params: &[p("period", 2.0, 400.0, 1.0, true), p("ema", 0.0, 1.0, 1.0, true)] },
    RuleSpec { rule: "x_macd_cross", role: Role::Exit, params: &[p("fast", 2.0, 100.0, 1.0, true), p("slow", 3.0, 200.0, 1.0, true), p("signal", 2.0, 50.0, 1.0, true)] },
    RuleSpec { rule: "x_macd_hist", role: Role::Exit, params: &[p("fast", 2.0, 100.0, 1.0, true), p("slow", 3.0, 200.0, 1.0, true), p("signal", 2.0, 50.0, 1.0, true), p("n", 1.0, 20.0, 1.0, true)] },
    RuleSpec { rule: "x_rsi_cross", role: Role::Exit, params: &[p("period", 2.0, 100.0, 1.0, true), p("level", 1.0, 99.0, 1.0, false)] },
    RuleSpec { rule: "x_bb_mid", role: Role::Exit, params: &[p("period", 5.0, 200.0, 1.0, true), p("k", 0.5, 5.0, 0.1, false)] },
    RuleSpec { rule: "x_stoch", role: Role::Exit, params: &[p("k", 3.0, 100.0, 1.0, true), p("d", 1.0, 20.0, 1.0, true), p("level", 50.0, 99.0, 1.0, false)] },
    RuleSpec { rule: "x_time", role: Role::Exit, params: &[p("bars", 1.0, 1000.0, 1.0, true)] },
    RuleSpec { rule: "x_against", role: Role::Exit, params: &[p("n", 1.0, 20.0, 1.0, true)] },
    RuleSpec { rule: "x_gap", role: Role::Exit, params: &[p("pct", 0.1, 50.0, 0.1, false)] },
];

/// A named signal: a rule plus its parameters.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SignalSpec {
    /// Free label, echoed back. Presets use their catalog id.
    pub id: String,
    /// One of the catalog rules (GET /api/journal/exits/catalog).
    pub rule: String,
    /// Missing keys take the preset default of that rule; unknown keys are refused.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

/// The default run: 30 simple, common exits.
const PRESETS: &[(&str, &str, &[(&str, f64)])] = &[
    ("sl_pct_2", "sl_pct", &[("pct", 2.0)]),
    ("sl_pct_5", "sl_pct", &[("pct", 5.0)]),
    ("sl_atr_1_5", "sl_atr", &[("mult", 1.5), ("period", 14.0)]),
    ("sl_atr_2", "sl_atr", &[("mult", 2.0), ("period", 14.0)]),
    ("sl_trail_pct_3", "sl_trail_pct", &[("pct", 3.0)]),
    ("sl_trail_atr_3", "sl_trail_atr", &[("mult", 3.0), ("period", 14.0)]),
    ("sl_donchian_10", "sl_donchian", &[("n", 10.0)]),
    ("sl_swing_5", "sl_swing", &[("n", 5.0)]),
    ("sl_planned", "sl_planned", &[]),
    ("tp_pct_3", "tp_pct", &[("pct", 3.0)]),
    ("tp_atr_3", "tp_atr", &[("mult", 3.0), ("period", 14.0)]),
    ("tp_r_1", "tp_r", &[("r", 1.0)]),
    ("tp_r_2", "tp_r", &[("r", 2.0)]),
    ("tp_r_3", "tp_r", &[("r", 3.0)]),
    ("tp_rsi_70", "tp_rsi", &[("period", 14.0), ("level", 70.0)]),
    ("tp_rsi_80", "tp_rsi", &[("period", 14.0), ("level", 80.0)]),
    ("tp_bb_20_2", "tp_bb", &[("period", 20.0), ("k", 2.0)]),
    ("tp_donchian_20", "tp_donchian", &[("n", 20.0)]),
    ("x_sma_10_30", "x_ma_cross", &[("fast", 10.0), ("slow", 30.0), ("ema", 0.0)]),
    ("x_ema_9_21", "x_ma_cross", &[("fast", 9.0), ("slow", 21.0), ("ema", 1.0)]),
    ("x_close_sma_20", "x_close_ma", &[("period", 20.0), ("ema", 0.0)]),
    ("x_close_ema_10", "x_close_ma", &[("period", 10.0), ("ema", 1.0)]),
    ("x_macd_cross", "x_macd_cross", &[("fast", 12.0), ("slow", 26.0), ("signal", 9.0)]),
    ("x_macd_hist_3", "x_macd_hist", &[("fast", 12.0), ("slow", 26.0), ("signal", 9.0), ("n", 3.0)]),
    ("x_rsi_70", "x_rsi_cross", &[("period", 14.0), ("level", 70.0)]),
    ("x_rsi_50", "x_rsi_cross", &[("period", 14.0), ("level", 50.0)]),
    ("x_bb_mid", "x_bb_mid", &[("period", 20.0), ("k", 2.0)]),
    ("x_stoch_80", "x_stoch", &[("k", 14.0), ("d", 3.0), ("level", 80.0)]),
    ("x_time_10", "x_time", &[("bars", 10.0)]),
    ("x_against_3", "x_against", &[("n", 3.0)]),
];

fn rule_spec(rule: &str) -> Option<&'static RuleSpec> {
    RULES.iter().find(|r| r.rule == rule)
}

/// The presets as signal specs, in catalog order.
pub fn presets() -> Vec<SignalSpec> {
    PRESETS
        .iter()
        .map(|(id, rule, params)| SignalSpec {
            id: id.to_string(),
            rule: rule.to_string(),
            params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        })
        .collect()
}

/// A signal checked against its rule: every parameter present, inside its range, integers
/// rounded. The first preset of the rule supplies what the caller left out.
#[derive(Debug, Clone)]
struct Signal {
    id: String,
    rule: &'static RuleSpec,
    params: BTreeMap<String, f64>,
}

impl Signal {
    fn get(&self, k: &str) -> f64 {
        self.params.get(k).copied().unwrap_or(0.0)
    }
    fn n(&self, k: &str) -> usize {
        self.get(k).max(0.0) as usize
    }
}

fn validate(spec: &SignalSpec) -> Result<Signal> {
    let rule = rule_spec(&spec.rule).ok_or_else(|| anyhow!("unknown rule '{}'", spec.rule))?;
    let defaults: BTreeMap<String, f64> = PRESETS
        .iter()
        .find(|(_, r, _)| *r == rule.rule)
        .map(|(_, _, ps)| ps.iter().map(|(k, v)| (k.to_string(), *v)).collect())
        .unwrap_or_default();
    for k in spec.params.keys() {
        if !rule.params.iter().any(|p| p.key == k) {
            return Err(anyhow!("rule '{}' has no parameter '{}'", rule.rule, k));
        }
    }
    let mut params = BTreeMap::new();
    for ps in rule.params {
        let v = spec
            .params
            .get(ps.key)
            .or_else(|| defaults.get(ps.key))
            .copied()
            .ok_or_else(|| anyhow!("rule '{}' needs '{}'", rule.rule, ps.key))?;
        if !v.is_finite() || v < ps.min || v > ps.max {
            return Err(anyhow!(
                "{}.{} = {} is outside [{}, {}]",
                rule.rule, ps.key, v, ps.min, ps.max
            ));
        }
        params.insert(ps.key.to_string(), if ps.integer { v.round() } else { v });
    }
    if matches!(rule.rule, "x_ma_cross" | "x_macd_cross" | "x_macd_hist")
        && params["fast"] >= params["slow"]
    {
        return Err(anyhow!("{}: fast must be below slow", rule.rule));
    }
    let id = spec.id.trim();
    Ok(Signal {
        id: if id.is_empty() { rule.rule.to_string() } else { id.to_string() },
        rule,
        params,
    })
}

// ── Indicators ───────────────────────────────────────────────────────────────

type Series = Vec<Option<f64>>;

fn sma(x: &[f64], n: usize) -> Series {
    let mut out = vec![None; x.len()];
    if n == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..x.len() {
        sum += x[i];
        if i >= n {
            sum -= x[i - n];
        }
        if i + 1 >= n {
            out[i] = Some(sum / n as f64);
        }
    }
    out
}

/// EMA seeded with the SMA of its first `n` defined values, so an input series with a
/// leading gap (a MACD line) is handled the same way as a price series.
fn ema_opt(x: &Series, n: usize) -> Series {
    let mut out = vec![None; x.len()];
    if n == 0 {
        return out;
    }
    let k = 2.0 / (n as f64 + 1.0);
    let mut seed: Vec<f64> = Vec::with_capacity(n);
    let mut prev: Option<f64> = None;
    for (i, v) in x.iter().enumerate() {
        let Some(v) = *v else { continue };
        match prev {
            Some(p) => {
                let e = v * k + p * (1.0 - k);
                out[i] = Some(e);
                prev = Some(e);
            }
            None => {
                seed.push(v);
                if seed.len() == n {
                    let e = seed.iter().sum::<f64>() / n as f64;
                    out[i] = Some(e);
                    prev = Some(e);
                }
            }
        }
    }
    out
}

fn ema(x: &[f64], n: usize) -> Series {
    ema_opt(&x.iter().map(|v| Some(*v)).collect(), n)
}

/// Wilder RSI.
fn rsi(c: &[f64], n: usize) -> Series {
    let mut out = vec![None; c.len()];
    if n == 0 || c.len() <= n {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=n {
        let d = c[i] - c[i - 1];
        if d > 0.0 { gain += d } else { loss -= d }
    }
    gain /= n as f64;
    loss /= n as f64;
    let val = |g: f64, l: f64| if l == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + g / l) };
    out[n] = Some(val(gain, loss));
    for i in n + 1..c.len() {
        let d = c[i] - c[i - 1];
        gain = (gain * (n - 1) as f64 + d.max(0.0)) / n as f64;
        loss = (loss * (n - 1) as f64 + (-d).max(0.0)) / n as f64;
        out[i] = Some(val(gain, loss));
    }
    out
}

/// Wilder ATR.
fn atr(b: &Bars, n: usize) -> Series {
    let len = b.c.len();
    let mut out = vec![None; len];
    if n == 0 || len <= n {
        return out;
    }
    let tr = |i: usize| {
        (b.h[i] - b.l[i])
            .max((b.h[i] - b.c[i - 1]).abs())
            .max((b.l[i] - b.c[i - 1]).abs())
    };
    let mut a = (1..=n).map(tr).sum::<f64>() / n as f64;
    out[n] = Some(a);
    for i in n + 1..len {
        a = (a * (n - 1) as f64 + tr(i)) / n as f64;
        out[i] = Some(a);
    }
    out
}

fn stdev(x: &[f64], n: usize) -> Series {
    let mean = sma(x, n);
    (0..x.len())
        .map(|i| {
            let m = mean[i]?;
            let var = x[i + 1 - n..=i].iter().map(|v| (v - m).powi(2)).sum::<f64>() / n as f64;
            Some(var.sqrt())
        })
        .collect()
}

/// Lowest low / highest high of the `n` bars *before* `i` (the current bar excluded, so the
/// channel is a level the bar can break, not one it defines).
fn channel(x: &[f64], n: usize, low: bool) -> Series {
    (0..x.len())
        .map(|i| {
            if n == 0 || i < n {
                return None;
            }
            let w = &x[i - n..i];
            Some(if low {
                w.iter().copied().fold(f64::INFINITY, f64::min)
            } else {
                w.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            })
        })
        .collect()
}

fn stoch(b: &Bars, k: usize, d: usize) -> (Series, Series) {
    let len = b.c.len();
    let mut pk: Series = vec![None; len];
    for i in 0..len {
        if k == 0 || i + 1 < k {
            continue;
        }
        let lo = b.l[i + 1 - k..=i].iter().copied().fold(f64::INFINITY, f64::min);
        let hi = b.h[i + 1 - k..=i].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        pk[i] = Some(if hi > lo { (b.c[i] - lo) / (hi - lo) * 100.0 } else { 50.0 });
    }
    let mut pd: Series = vec![None; len];
    for i in 0..len {
        if d == 0 || i + 1 < d {
            continue;
        }
        let w: Option<Vec<f64>> = pk[i + 1 - d..=i].iter().copied().collect();
        pd[i] = w.map(|w| w.iter().sum::<f64>() / d as f64);
    }
    (pk, pd)
}

// ── Bars and trades ──────────────────────────────────────────────────────────

/// One trade's candles, column-wise.
struct Bars {
    ts: Vec<OffsetDateTime>,
    o: Vec<f64>,
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
}

/// Everything the rules need about one trade.
struct Ctx {
    bars: Bars,
    /// First bar of the position (the first stamped at or after the entry).
    start: usize,
    /// Bar holding the real exit.
    real: usize,
    /// Last bar a rule may fire on (the real exit in overlay mode, the extension otherwise).
    end: usize,
    entry: f64,
    /// +1 long, -1 short.
    dir: f64,
    stop: Option<f64>,
    cache: HashMap<String, Series>,
}

impl Ctx {
    fn series(&mut self, key: String, f: impl FnOnce(&Bars) -> Series) -> Series {
        if let Some(s) = self.cache.get(&key) {
            return s.clone();
        }
        let s = f(&self.bars);
        self.cache.insert(key, s.clone());
        s
    }
    fn ma(&mut self, n: usize, exp: bool) -> Series {
        let key = format!("{}{n}", if exp { "ema" } else { "sma" });
        self.series(key, |b| if exp { ema(&b.c, n) } else { sma(&b.c, n) })
    }
    fn rsi(&mut self, n: usize) -> Series {
        self.series(format!("rsi{n}"), |b| rsi(&b.c, n))
    }
    fn atr_at_entry(&mut self, n: usize) -> Option<f64> {
        let a = self.series(format!("atr{n}"), |b| atr(b, n));
        // The last value known before the position opened.
        self.start.checked_sub(1).and_then(|i| a[i]).or(a[self.start])
    }
    fn macd(&mut self, fast: usize, slow: usize, sig: usize) -> (Series, Series) {
        let f = self.ma(fast, true);
        let s = self.ma(slow, true);
        let line: Series = f.iter().zip(&s).map(|(a, b)| Some((*a)? - (*b)?)).collect();
        let signal = ema_opt(&line, sig);
        (line, signal)
    }
}

/// When a rule closes the trade. `order` breaks ties inside one bar: a gap at the open,
/// then a stop, then a target, then anything read on the close.
#[derive(Debug, Clone, Copy)]
struct Fire {
    idx: usize,
    price: f64,
    order: u8,
}

const AT_OPEN: u8 = 0;
const AT_STOP: u8 = 1;
const AT_TARGET: u8 = 2;
const AT_CLOSE: u8 = 3;

/// A fixed level against the position: fills at the level, or at the open if the bar
/// gapped through it.
fn stop_hit(b: &Bars, i: usize, dir: f64, level: f64) -> Option<Fire> {
    if (level - b.o[i]) * dir >= 0.0 {
        return Some(Fire { idx: i, price: b.o[i], order: AT_OPEN });
    }
    let adverse = if dir > 0.0 { b.l[i] } else { b.h[i] };
    ((level - adverse) * dir >= 0.0).then_some(Fire { idx: i, price: level, order: AT_STOP })
}

/// A fixed level in favour of the position.
fn target_hit(b: &Bars, i: usize, dir: f64, level: f64) -> Option<Fire> {
    if (b.o[i] - level) * dir >= 0.0 {
        return Some(Fire { idx: i, price: b.o[i], order: AT_OPEN });
    }
    let fav = if dir > 0.0 { b.h[i] } else { b.l[i] };
    ((fav - level) * dir >= 0.0).then_some(Fire { idx: i, price: level, order: AT_TARGET })
}

fn close_fire(b: &Bars, i: usize) -> Fire {
    Fire { idx: i, price: b.c[i], order: AT_CLOSE }
}

/// `a` crossed `b` against the position on bar `i`: below for a long, above for a short.
fn crossed_against(a: &Series, b: &Series, i: usize, dir: f64) -> bool {
    if i == 0 {
        return false;
    }
    match (a[i - 1], b[i - 1], a[i], b[i]) {
        (Some(a0), Some(b0), Some(a1), Some(b1)) => (a0 - b0) * dir >= 0.0 && (a1 - b1) * dir < 0.0,
        _ => false,
    }
}

/// First bar in `[start, end]` where `f` fires.
fn scan(ctx: &Ctx, mut f: impl FnMut(usize) -> Option<Fire>) -> Option<Fire> {
    (ctx.start..=ctx.end).find_map(&mut f)
}

/// Where a rule closes the trade inside `[start, end]`. `Err(())` = the rule cannot apply
/// to this trade (it needs a planned stop the trade does not carry, or an ATR the history
/// cannot seed).
fn fire(sig: &Signal, ctx: &mut Ctx) -> Result<Option<Fire>, ()> {
    let dir = ctx.dir;
    let entry = ctx.entry;
    let level_from = |pct: f64, against: bool| {
        let s = if against { -dir } else { dir };
        entry * (1.0 + s * pct / 100.0)
    };
    let out = match sig.rule.rule {
        "sl_pct" => {
            let lv = level_from(sig.get("pct"), true);
            scan(ctx, |i| stop_hit(&ctx.bars, i, dir, lv))
        }
        "sl_atr" => {
            let a = ctx.atr_at_entry(sig.n("period")).ok_or(())?;
            let lv = entry - dir * sig.get("mult") * a;
            scan(ctx, |i| stop_hit(&ctx.bars, i, dir, lv))
        }
        "sl_planned" => {
            let lv = ctx.stop.ok_or(())?;
            scan(ctx, |i| stop_hit(&ctx.bars, i, dir, lv))
        }
        "sl_swing" => {
            let n = sig.n("n");
            if ctx.start < n {
                return Err(());
            }
            let w = ctx.start - n..ctx.start;
            let lv = if dir > 0.0 {
                ctx.bars.l[w].iter().copied().fold(f64::INFINITY, f64::min)
            } else {
                ctx.bars.h[w].iter().copied().fold(f64::NEG_INFINITY, f64::max)
            };
            // A swing already beyond the entry is not a stop.
            if (entry - lv) * dir <= 0.0 {
                return Err(());
            }
            scan(ctx, |i| stop_hit(&ctx.bars, i, dir, lv))
        }
        "sl_trail_pct" | "sl_trail_atr" => {
            let dist = if sig.rule.rule == "sl_trail_pct" {
                None
            } else {
                Some(sig.get("mult") * ctx.atr_at_entry(sig.n("period")).ok_or(())?)
            };
            let pct = sig.get("pct") / 100.0;
            // Best price seen before the bar being tested.
            let mut peak = entry;
            let b = &ctx.bars;
            let mut hit = None;
            for i in ctx.start..=ctx.end {
                let lv = match dist {
                    Some(d) => peak - dir * d,
                    None => peak * (1.0 - dir * pct),
                };
                if let Some(f) = stop_hit(b, i, dir, lv) {
                    hit = Some(f);
                    break;
                }
                let fav = if dir > 0.0 { b.h[i] } else { b.l[i] };
                if (fav - peak) * dir > 0.0 {
                    peak = fav;
                }
            }
            hit
        }
        "sl_donchian" => {
            let n = sig.n("n");
            let src = if dir > 0.0 { &ctx.bars.l } else { &ctx.bars.h };
            let ch = channel(src, n, dir > 0.0);
            scan(ctx, |i| stop_hit(&ctx.bars, i, dir, ch[i]?))
        }
        "tp_pct" => {
            let lv = level_from(sig.get("pct"), false);
            scan(ctx, |i| target_hit(&ctx.bars, i, dir, lv))
        }
        "tp_atr" => {
            let a = ctx.atr_at_entry(sig.n("period")).ok_or(())?;
            let lv = entry + dir * sig.get("mult") * a;
            scan(ctx, |i| target_hit(&ctx.bars, i, dir, lv))
        }
        "tp_r" => {
            let stop = ctx.stop.ok_or(())?;
            let risk = (entry - stop).abs();
            if risk <= 0.0 {
                return Err(());
            }
            let lv = entry + dir * sig.get("r") * risk;
            scan(ctx, |i| target_hit(&ctx.bars, i, dir, lv))
        }
        "tp_rsi" => {
            let r = ctx.rsi(sig.n("period"));
            let lvl = if dir > 0.0 { sig.get("level") } else { 100.0 - sig.get("level") };
            scan(ctx, |i| ((r[i]? - lvl) * dir >= 0.0).then(|| close_fire(&ctx.bars, i)))
        }
        "tp_bb" => {
            let n = sig.n("period");
            let k = sig.get("k");
            let mid = ctx.ma(n, false);
            let sd = ctx.series(format!("sd{n}"), |b| stdev(&b.c, n));
            // The previous bar's band: the current one is not known until its close.
            scan(ctx, |i| {
                let j = i.checked_sub(1)?;
                let lv = mid[j]? + dir * k * sd[j]?;
                target_hit(&ctx.bars, i, dir, lv)
            })
        }
        "tp_donchian" => {
            let n = sig.n("n");
            let src = if dir > 0.0 { &ctx.bars.h } else { &ctx.bars.l };
            let ch = channel(src, n, dir < 0.0);
            scan(ctx, |i| target_hit(&ctx.bars, i, dir, ch[i]?))
        }
        "x_ma_cross" => {
            let exp = sig.get("ema") >= 0.5;
            let f = ctx.ma(sig.n("fast"), exp);
            let s = ctx.ma(sig.n("slow"), exp);
            scan(ctx, |i| crossed_against(&f, &s, i, dir).then(|| close_fire(&ctx.bars, i)))
        }
        "x_close_ma" => {
            let m = ctx.ma(sig.n("period"), sig.get("ema") >= 0.5);
            let c: Series = ctx.bars.c.iter().map(|v| Some(*v)).collect();
            scan(ctx, |i| crossed_against(&c, &m, i, dir).then(|| close_fire(&ctx.bars, i)))
        }
        "x_macd_cross" => {
            let (line, signal) = ctx.macd(sig.n("fast"), sig.n("slow"), sig.n("signal"));
            scan(ctx, |i| crossed_against(&line, &signal, i, dir).then(|| close_fire(&ctx.bars, i)))
        }
        "x_macd_hist" => {
            let (line, signal) = ctx.macd(sig.n("fast"), sig.n("slow"), sig.n("signal"));
            let hist: Series = line.iter().zip(&signal).map(|(a, b)| Some((*a)? - (*b)?)).collect();
            let n = sig.n("n");
            // `n` consecutive bars of the histogram moving against the position, counted
            // from the entry so a fade already running at entry does not close it at once.
            let mut run = 0usize;
            scan(ctx, |i| {
                let fading = i > 0
                    && matches!((hist[i - 1], hist[i]), (Some(a), Some(b)) if (b - a) * dir < 0.0);
                run = if fading { run + 1 } else { 0 };
                (run >= n).then(|| close_fire(&ctx.bars, i))
            })
        }
        "x_rsi_cross" => {
            let r = ctx.rsi(sig.n("period"));
            let lvl = if dir > 0.0 { sig.get("level") } else { 100.0 - sig.get("level") };
            let l: Series = vec![Some(lvl); r.len()];
            scan(ctx, |i| crossed_against(&r, &l, i, dir).then(|| close_fire(&ctx.bars, i)))
        }
        "x_bb_mid" => {
            let m = ctx.ma(sig.n("period"), false);
            let c: Series = ctx.bars.c.iter().map(|v| Some(*v)).collect();
            scan(ctx, |i| crossed_against(&c, &m, i, dir).then(|| close_fire(&ctx.bars, i)))
        }
        "x_stoch" => {
            let (kn, dn) = (sig.n("k"), sig.n("d"));
            let (pk, pd) = {
                let b = &ctx.bars;
                let key = format!("stoch{kn}_{dn}");
                if !ctx.cache.contains_key(&key) {
                    let (a, d) = stoch(b, kn, dn);
                    ctx.cache.insert(key.clone(), a);
                    ctx.cache.insert(format!("{key}d"), d);
                }
                (ctx.cache[&key].clone(), ctx.cache[&format!("{key}d")].clone())
            };
            let lvl = if dir > 0.0 { sig.get("level") } else { 100.0 - sig.get("level") };
            scan(ctx, |i| {
                let stretched = i > 0 && (pk[i - 1]? - lvl) * dir >= 0.0;
                (stretched && crossed_against(&pk, &pd, i, dir)).then(|| close_fire(&ctx.bars, i))
            })
        }
        "x_time" => {
            let i = ctx.start + sig.n("bars").saturating_sub(1);
            (i <= ctx.end).then(|| close_fire(&ctx.bars, i))
        }
        "x_against" => {
            let n = sig.n("n");
            let mut run = 0usize;
            scan(ctx, |i| {
                let prev = if i == ctx.start { entry } else { ctx.bars.c[i - 1] };
                run = if (ctx.bars.c[i] - prev) * dir < 0.0 { run + 1 } else { 0 };
                (run >= n).then(|| close_fire(&ctx.bars, i))
            })
        }
        "x_gap" => {
            let pct = sig.get("pct") / 100.0;
            scan(ctx, |i| {
                if i <= ctx.start {
                    return None;
                }
                let pc = ctx.bars.c[i - 1];
                (pc > 0.0 && (ctx.bars.o[i] - pc) / pc * dir <= -pct)
                    .then_some(Fire { idx: i, price: ctx.bars.o[i], order: AT_OPEN })
            })
        }
        other => unreachable!("rule '{other}' is in the catalog but not implemented"),
    };
    Ok(out)
}

// ── Run ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Overlay,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Total net PnL.
    #[default]
    Net,
    /// Smallest max drawdown of the cumulative PnL.
    Drawdown,
    WinRate,
    AvgR,
    ProfitFactor,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SimulateBody {
    /// Closed trades to replay (max 500). Their candles must already be in store
    /// (journal Market data: sync + compute).
    pub trade_ids: Vec<Uuid>,
    /// Signals to test. Omitted = the 30 catalog presets.
    #[serde(default)]
    pub signals: Option<Vec<SignalSpec>>,
    /// overlay (default): a rule can only close a trade before its real exit.
    /// replace: the rule is the exit and may hold up to `extend_bars` past it.
    #[serde(default)]
    pub mode: Mode,
    /// Candles allowed past the real exit in replace mode (default 20, max 500).
    #[serde(default)]
    pub extend_bars: Option<usize>,
    /// Ranking criterion for `best` and the order of `results` (default net).
    #[serde(default)]
    pub objective: Objective,
    /// Also test the best stop combined with the best target/exit (default true).
    #[serde(default)]
    pub combo: Option<bool>,
    /// "summary" drops the per-trade rows (use it over MCP); default "full".
    #[serde(default)]
    pub view: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TradeInfo {
    pub id: Uuid,
    pub ticker: String,
    pub side: String,
    pub currency: String,
    #[serde(with = "time::serde::rfc3339")]
    pub entry_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub exit_at: OffsetDateTime,
    /// Real net PnL, in the display currency.
    pub net: f64,
    pub r: Option<f64>,
    pub hold_bars: usize,
    /// Candles available before the entry to seed the indicators.
    pub warmup_bars: usize,
    /// Candles available after the real exit (replace mode only reads them).
    pub after_bars: usize,
}

#[derive(Debug, Serialize)]
pub struct Skipped {
    pub id: Uuid,
    pub ticker: String,
    /// open | undated | unsupported | unmapped | no_bars | thin | no_price | fx
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Earlier,
    Later,
    Unchanged,
    NotApplicable,
}

#[derive(Debug, Serialize)]
pub struct TradeSim {
    pub trade_id: Uuid,
    pub outcome: Outcome,
    /// Stamp of the bar the simulated exit happened in (unchanged = the real exit).
    #[serde(with = "time::serde::rfc3339::option")]
    pub exit_bar: Option<OffsetDateTime>,
    pub exit_price: Option<f64>,
    pub net: f64,
    pub r: Option<f64>,
    pub hold_bars: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Stats {
    pub trades: usize,
    pub earlier: usize,
    pub later: usize,
    pub unchanged: usize,
    pub not_applicable: usize,
    pub net: f64,
    /// Against the real result.
    pub delta: f64,
    pub win_rate: f64,
    pub avg_r: Option<f64>,
    pub profit_factor: Option<f64>,
    pub max_drawdown: f64,
    pub improved: usize,
    pub worsened: usize,
    pub avg_hold_bars: f64,
    /// Cumulative net after each trade, in exit order of the real trades.
    pub curve: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct SignalResult {
    pub id: String,
    pub rule: &'static str,
    pub role: Role,
    pub params: BTreeMap<String, f64>,
    pub stats: Stats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_trade: Option<Vec<TradeSim>>,
}

#[derive(Debug, Serialize)]
pub struct Best {
    pub sl: Option<String>,
    pub tp: Option<String>,
    pub combo: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub currency: String,
    pub mode: Mode,
    pub objective: Objective,
    pub extend_bars: usize,
    pub trades: Vec<TradeInfo>,
    pub skipped: Vec<Skipped>,
    pub baseline: Stats,
    /// Sorted best first by `objective`.
    pub results: Vec<SignalResult>,
    pub best: Best,
}

/// A trade ready to replay.
struct Loaded {
    trade: Trade,
    ctx: Ctx,
    /// Trade currency → display currency.
    fx: f64,
    /// Entry quantity × multiplier.
    unit: f64,
    fees: f64,
    risk: Option<f64>,
    timeframe: String,
}

struct LoadOut {
    loaded: Vec<Loaded>,
    skipped: Vec<Skipped>,
}

/// Load the trades and their candles. `extend` is how many candles past the real exit are
/// read (and made reachable to the rules).
/// `currency` = None skips the FX pass (the chart shows prices, not money).
async fn load(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
    extend: usize,
    currency: Option<&str>,
) -> Result<LoadOut> {
    let settings = market_store::get_settings(pool).await?;
    let granted = conn_store::list_for_module(pool, journal_market::MODULE).await?;
    let mut trades = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(t) = journal::get_trade(pool, *id).await? {
            trades.push(t);
        }
    }
    // Same grain as the enrichment, so the lab reads the series the sync downloaded.
    let timeframe = journal_market::resolve_timeframe(&settings.timeframe, &trades);
    let tf_secs = histdata::timeframe_secs(&timeframe)?;
    let bar = Duration::seconds(tf_secs);

    let mut fx = otw_store::journal_fx::FxCache::new();
    let mut datasets: HashMap<(String, String), Option<Uuid>> = HashMap::new();
    let mut out = LoadOut { loaded: Vec::new(), skipped: Vec::new() };

    for t in trades {
        let skip = |t: &Trade, reason| Skipped { id: t.id, ticker: t.ticker.clone(), reason };
        if t.net_pnl.is_none() || t.open_qty > 0.0 {
            out.skipped.push(skip(&t, "open"));
            continue;
        }
        let Some(asset_type) = journal_market::asset_type_for(&t.asset_class) else {
            out.skipped.push(skip(&t, "unsupported"));
            continue;
        };
        let Some((entry_at, exit_at)) = trade_window(&t) else {
            out.skipped.push(skip(&t, "undated"));
            continue;
        };
        let Some(ticker) = provider_symbol(&settings.symbols, &t.asset_class, &t.ticker) else {
            out.skipped.push(skip(&t, "unmapped"));
            continue;
        };
        let key = (asset_type.to_string(), ticker.clone());
        let dataset = match datasets.get(&key) {
            Some(d) => *d,
            None => {
                let preferred = pick_connector(&granted, &settings.connectors, asset_type)
                    .map(|c| c.provider.clone());
                let d = store::find_dataset_any(pool, asset_type, &ticker, &timeframe, preferred.as_deref())
                    .await?
                    .map(|d| d.id);
                datasets.insert(key, d);
                d
            }
        };
        let Some(dataset_id) = dataset else {
            out.skipped.push(skip(&t, "no_bars"));
            continue;
        };
        let (Some(entry), qty) = (t.avg_entry, journal::trade_entry_qty(&t).abs()) else {
            out.skipped.push(skip(&t, "no_price"));
            continue;
        };
        if entry <= 0.0 || qty <= 0.0 {
            out.skipped.push(skip(&t, "no_price"));
            continue;
        }
        let raw = store::read_bars(
            pool,
            dataset_id,
            Some(entry_at - bar * WARMUP_BARS as i32),
            Some(exit_at + bar * extend as i32),
            MAX_BARS_PER_TRADE,
        )
        .await?;
        let bars = Bars {
            ts: raw.iter().map(|b| b.ts).collect(),
            o: raw.iter().map(|b| b.open).collect(),
            h: raw.iter().map(|b| b.high).collect(),
            l: raw.iter().map(|b| b.low).collect(),
            c: raw.iter().map(|b| b.close).collect(),
        };
        // A bar is a period: the one the entry fell in belongs to the position, and the
        // real exit sits in the last bar that opened at or before it.
        let start = bars.ts.partition_point(|ts| *ts + bar <= entry_at);
        let real_end = bars.ts.partition_point(|ts| *ts <= exit_at);
        if start >= bars.ts.len() || real_end <= start {
            out.skipped.push(skip(&t, "thin"));
            continue;
        }
        let real = real_end - 1;
        let rate = match currency {
            Some(cur) => fx.convert(pool, 1.0, &t.currency, cur, exit_at.date()).await?,
            None => Some(1.0),
        };
        let Some(rate) = rate else {
            out.skipped.push(skip(&t, "fx"));
            continue;
        };
        let dir = if t.side.eq_ignore_ascii_case("short") { -1.0 } else { 1.0 };
        let last = bars.ts.len() - 1;
        let ctx = Ctx {
            bars,
            start,
            real,
            end: (real + extend).min(last),
            entry,
            dir,
            stop: trade_stop(&t),
            cache: HashMap::new(),
        };
        out.loaded.push(Loaded {
            unit: qty * t.multiplier,
            fees: journal::trade_total_fees(&t),
            risk: trade_risk(&t),
            fx: rate,
            trade: t,
            ctx,
            timeframe: timeframe.clone(),
        });
    }
    // Exit order, so the curves and drawdowns read like the account did.
    out.loaded.sort_by_key(|l| trade_window(&l.trade).map(|w| w.1));
    Ok(out)
}

/// Apply the run's semantics to a raw fire and price the trade.
fn settle(l: &Loaded, fire: Result<Option<Fire>, ()>, may_extend: bool) -> TradeSim {
    let real_net = l.trade.net_pnl.unwrap_or(0.0) * l.fx;
    let r_of = |net_ccy: f64| l.risk.map(|r| net_ccy / r);
    let ctx = &l.ctx;
    let unchanged = |outcome| TradeSim {
        trade_id: l.trade.id,
        outcome,
        exit_bar: Some(ctx.bars.ts[ctx.real]),
        exit_price: journal::trade_avg_exit(&l.trade),
        net: real_net,
        r: r_of(l.trade.net_pnl.unwrap_or(0.0)),
        hold_bars: ctx.real - ctx.start + 1,
    };
    let f = match fire {
        Err(()) => return unchanged(Outcome::NotApplicable),
        Ok(None) => return unchanged(Outcome::Unchanged),
        Ok(Some(f)) => f,
    };
    if f.idx > ctx.real && !may_extend {
        return unchanged(Outcome::Unchanged);
    }
    let net_ccy = (f.price - ctx.entry) * ctx.dir * l.unit - l.fees;
    TradeSim {
        trade_id: l.trade.id,
        outcome: if f.idx <= ctx.real { Outcome::Earlier } else { Outcome::Later },
        exit_bar: Some(ctx.bars.ts[f.idx]),
        exit_price: Some(f.price),
        net: net_ccy * l.fx,
        r: r_of(net_ccy),
        hold_bars: f.idx - ctx.start + 1,
    }
}

fn stats(rows: &[TradeSim], real: &[f64]) -> Stats {
    let mut s = Stats { trades: rows.len(), ..Default::default() };
    let (mut gross_win, mut gross_loss, mut wins) = (0.0, 0.0, 0usize);
    let (mut r_sum, mut r_n) = (0.0, 0usize);
    let (mut cum, mut peak) = (0.0f64, 0.0f64);
    let mut hold = 0usize;
    for (row, real) in rows.iter().zip(real) {
        match row.outcome {
            Outcome::Earlier => s.earlier += 1,
            Outcome::Later => s.later += 1,
            Outcome::Unchanged => s.unchanged += 1,
            Outcome::NotApplicable => s.not_applicable += 1,
        }
        s.net += row.net;
        if row.net > 0.0 {
            wins += 1;
            gross_win += row.net;
        } else {
            gross_loss -= row.net;
        }
        if let Some(r) = row.r {
            r_sum += r;
            r_n += 1;
        }
        let d = row.net - real;
        if d > 1e-9 {
            s.improved += 1;
        } else if d < -1e-9 {
            s.worsened += 1;
        }
        hold += row.hold_bars;
        cum += row.net;
        peak = peak.max(cum);
        s.max_drawdown = s.max_drawdown.max(peak - cum);
        s.curve.push(cum);
    }
    let n = rows.len().max(1) as f64;
    s.delta = s.net - real.iter().sum::<f64>();
    s.win_rate = wins as f64 / n * 100.0;
    s.avg_r = (r_n > 0).then(|| r_sum / r_n as f64);
    s.profit_factor = (gross_loss > 0.0).then(|| gross_win / gross_loss);
    s.avg_hold_bars = hold as f64 / n;
    s
}

/// Higher is better for every objective.
fn score(s: &Stats, o: Objective) -> f64 {
    match o {
        Objective::Net => s.net,
        Objective::Drawdown => -s.max_drawdown,
        Objective::WinRate => s.win_rate,
        Objective::AvgR => s.avg_r.unwrap_or(f64::NEG_INFINITY),
        Objective::ProfitFactor => s.profit_factor.unwrap_or(f64::INFINITY),
    }
}

pub async fn simulate(pool: &sqlx::PgPool, body: SimulateBody, currency: &str) -> Result<Report> {
    if body.trade_ids.is_empty() {
        return Err(anyhow!("no trade selected"));
    }
    if body.trade_ids.len() > MAX_TRADES {
        return Err(anyhow!("at most {MAX_TRADES} trades per run"));
    }
    let specs = body.signals.unwrap_or_else(presets);
    if specs.is_empty() || specs.len() > MAX_SIGNALS {
        return Err(anyhow!("between 1 and {MAX_SIGNALS} signals per run"));
    }
    let signals = specs.iter().map(validate).collect::<Result<Vec<_>>>()?;
    let extend = match body.mode {
        Mode::Overlay => 0,
        Mode::Replace => body.extend_bars.unwrap_or(DEFAULT_EXTEND).min(MAX_EXTEND),
    };
    let full = body.view.as_deref() != Some("summary");
    let replace = body.mode == Mode::Replace;

    let LoadOut { mut loaded, skipped } = load(pool, &body.trade_ids, extend, Some(currency)).await?;
    let real: Vec<f64> = loaded.iter().map(|l| l.trade.net_pnl.unwrap_or(0.0) * l.fx).collect();

    // The real trades, run through the same stats so the baseline is built like a result.
    let base_rows: Vec<TradeSim> = loaded.iter().map(|l| settle(l, Ok(None), false)).collect();
    let baseline = stats(&base_rows, &real);

    // Raw fires per signal per trade, kept for the combination.
    let mut fires: Vec<Vec<Result<Option<Fire>, ()>>> = Vec::with_capacity(signals.len());
    let mut results = Vec::with_capacity(signals.len() + 1);
    for sig in &signals {
        let raw: Vec<_> = loaded.iter_mut().map(|l| fire(sig, &mut l.ctx)).collect();
        let may_extend = replace && sig.rule.role != Role::Sl;
        let rows: Vec<TradeSim> = loaded
            .iter()
            .zip(&raw)
            .map(|(l, f)| settle(l, *f, may_extend))
            .collect();
        results.push(SignalResult {
            id: sig.id.clone(),
            rule: sig.rule.rule,
            role: sig.rule.role,
            params: sig.params.clone(),
            stats: stats(&rows, &real),
            per_trade: full.then_some(rows),
        });
        fires.push(raw);
    }

    let best_of = |roles: &[Role]| -> Option<usize> {
        results
            .iter()
            .enumerate()
            .filter(|(_, r)| roles.contains(&r.role) && r.stats.earlier + r.stats.later > 0)
            .max_by(|a, b| score(&a.1.stats, body.objective).total_cmp(&score(&b.1.stats, body.objective)))
            .map(|(i, _)| i)
    };
    let best_sl = best_of(&[Role::Sl]);
    let best_tp = best_of(&[Role::Tp, Role::Exit]);

    let mut combo_id = None;
    if body.combo.unwrap_or(true) {
        if let (Some(a), Some(b)) = (best_sl, best_tp) {
            let id = format!("{} + {}", results[a].id, results[b].id);
            // Earliest of the two, the stop first inside one bar. A rule that cannot apply
            // leaves the other one alone.
            let rows: Vec<TradeSim> = loaded
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let pick = match (fires[a][i], fires[b][i]) {
                        (Err(()), Err(())) => Err(()),
                        (Ok(x), Err(())) | (Err(()), Ok(x)) => Ok(x),
                        (Ok(x), Ok(y)) => Ok(match (x, y) {
                            (Some(x), Some(y)) => Some(if (x.idx, x.order) <= (y.idx, y.order) { x } else { y }),
                            (x, y) => x.or(y),
                        }),
                    };
                    settle(l, pick, replace)
                })
                .collect();
            let mut params = BTreeMap::new();
            for (k, v) in &results[a].params {
                params.insert(format!("sl.{k}"), *v);
            }
            for (k, v) in &results[b].params {
                params.insert(format!("tp.{k}"), *v);
            }
            results.push(SignalResult {
                id: id.clone(),
                rule: "combo",
                role: Role::Sl,
                params,
                stats: stats(&rows, &real),
                per_trade: full.then_some(rows),
            });
            combo_id = Some(id);
        }
    }
    let best = Best {
        sl: best_sl.map(|i| results[i].id.clone()),
        tp: best_tp.map(|i| results[i].id.clone()),
        combo: combo_id,
    };
    results.sort_by(|a, b| score(&b.stats, body.objective).total_cmp(&score(&a.stats, body.objective)));

    let trades = loaded
        .iter()
        .zip(&base_rows)
        .map(|(l, row)| {
            let (entry_at, exit_at) = trade_window(&l.trade).expect("loaded trades are dated");
            TradeInfo {
                id: l.trade.id,
                ticker: l.trade.ticker.clone(),
                side: l.trade.side.clone(),
                currency: l.trade.currency.clone(),
                entry_at,
                exit_at,
                net: row.net,
                r: row.r,
                hold_bars: row.hold_bars,
                warmup_bars: l.ctx.start,
                after_bars: l.ctx.bars.ts.len() - 1 - l.ctx.real,
            }
        })
        .collect();
    Ok(Report {
        currency: currency.to_string(),
        mode: body.mode,
        objective: body.objective,
        extend_bars: extend,
        trades,
        skipped,
        baseline,
        results,
        best,
    })
}

// ── Chart ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TradeBars {
    pub timeframe: String,
    pub ts: Vec<i64>,
    pub o: Vec<f64>,
    pub h: Vec<f64>,
    pub l: Vec<f64>,
    pub c: Vec<f64>,
    pub entry_idx: usize,
    pub exit_idx: usize,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub stop: Option<f64>,
    pub side: String,
}

/// The candles of one trade, for the chart: the warm-up, the position and `extend` bars
/// past the exit, as far as the store holds them.
pub async fn trade_bars(pool: &sqlx::PgPool, id: Uuid, extend: usize) -> Result<Option<TradeBars>> {
    let out = load(pool, &[id], extend.min(MAX_EXTEND), None).await?;
    let Some(l) = out.loaded.into_iter().next() else {
        return Ok(None);
    };
    let b = &l.ctx.bars;
    Ok(Some(TradeBars {
        timeframe: l.timeframe.clone(),
        ts: b.ts.iter().map(|t| t.unix_timestamp()).collect(),
        o: b.o.clone(),
        h: b.h.clone(),
        l: b.l.clone(),
        c: b.c.clone(),
        entry_idx: l.ctx.start,
        exit_idx: l.ctx.real,
        entry_price: l.ctx.entry,
        exit_price: journal::trade_avg_exit(&l.trade),
        stop: l.ctx.stop,
        side: l.trade.side.clone(),
    }))
}

/// Why a trade has no chart, for the 404 message.
pub async fn why_no_bars(pool: &sqlx::PgPool, id: Uuid) -> Result<&'static str> {
    let out = load(pool, &[id], 0, None).await?;
    Ok(out.skipped.first().map(|s| s.reason).unwrap_or("not_found"))
}

/// A trade the lab can replay, as the picker lists it.
#[derive(Debug, Serialize)]
pub struct ReadyTrade {
    pub id: Uuid,
    pub ticker: String,
    pub side: String,
    pub currency: String,
    #[serde(with = "time::serde::rfc3339")]
    pub entry_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub exit_at: OffsetDateTime,
    pub net_pnl: f64,
}

#[derive(Debug, Serialize)]
pub struct Readiness {
    /// Closed trades in scope.
    pub closed: usize,
    /// The ones with a candle series in store, newest exit first.
    pub trades: Vec<ReadyTrade>,
}

/// Which trades of the scope the lab can replay: closed, dated, mapped and with a candle
/// series in store. Cheap: one dataset lookup per instrument, no bar read. A trade whose
/// own window the series does not cover is still listed and reported `thin` by the run.
pub async fn ready(pool: &sqlx::PgPool, filter: &TradeFilter) -> Result<Readiness> {
    let settings = market_store::get_settings(pool).await?;
    let granted = conn_store::list_for_module(pool, journal_market::MODULE).await?;
    let trades = journal::list_trades(pool, filter).await?;
    let timeframe = journal_market::resolve_timeframe(&settings.timeframe, &trades);
    let mut datasets: HashMap<(String, String), bool> = HashMap::new();
    let mut out = Readiness { closed: 0, trades: Vec::new() };
    for t in &trades {
        if t.net_pnl.is_none() || t.open_qty > 0.0 {
            continue;
        }
        out.closed += 1;
        let Some((entry_at, exit_at)) = trade_window(t) else { continue };
        let Some(asset_type) = journal_market::asset_type_for(&t.asset_class) else { continue };
        let Some(ticker) = provider_symbol(&settings.symbols, &t.asset_class, &t.ticker) else {
            continue;
        };
        let key = (asset_type.to_string(), ticker.clone());
        let has = match datasets.get(&key) {
            Some(h) => *h,
            None => {
                let preferred = pick_connector(&granted, &settings.connectors, asset_type)
                    .map(|c| c.provider.clone());
                let h = store::find_dataset_any(pool, asset_type, &ticker, &timeframe, preferred.as_deref())
                    .await?
                    .is_some_and(|d| d.bar_count > 0);
                datasets.insert(key, h);
                h
            }
        };
        if has {
            out.trades.push(ReadyTrade {
                id: t.id,
                ticker: t.ticker.clone(),
                side: t.side.clone(),
                currency: t.currency.clone(),
                entry_at,
                exit_at,
                net_pnl: t.net_pnl.unwrap_or(0.0),
            });
        }
    }
    out.trades.sort_by(|a, b| b.exit_at.cmp(&a.exit_at));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(c: &[f64]) -> Bars {
        let t0 = OffsetDateTime::UNIX_EPOCH;
        Bars {
            ts: (0..c.len()).map(|i| t0 + Duration::days(i as i64)).collect(),
            o: c.to_vec(),
            h: c.iter().map(|v| v + 1.0).collect(),
            l: c.iter().map(|v| v - 1.0).collect(),
            c: c.to_vec(),
        }
    }

    fn ctx(c: &[f64], start: usize, real: usize, dir: f64) -> Ctx {
        let entry = c[start];
        Ctx { bars: bars(c), start, real, end: real, entry, dir, stop: None, cache: HashMap::new() }
    }

    #[test]
    fn every_preset_validates_and_every_rule_is_implemented() {
        assert_eq!(PRESETS.len(), 30);
        let mut c = ctx(&(0..80).map(|i| 100.0 + (i as f64 * 0.3).sin() * 5.0).collect::<Vec<_>>(), 40, 70, 1.0);
        for spec in presets() {
            let s = validate(&spec).unwrap();
            let _ = fire(&s, &mut c);
        }
    }

    #[test]
    fn stop_fills_at_level_or_gap_open() {
        let b = bars(&[100.0, 99.0, 90.0]);
        let f = stop_hit(&b, 1, 1.0, 98.5).unwrap();
        assert_eq!((f.price, f.order), (98.5, AT_STOP));
        let f = stop_hit(&b, 2, 1.0, 95.0).unwrap();
        assert_eq!((f.price, f.order), (90.0, AT_OPEN));
        // Short: the stop is above.
        assert!(stop_hit(&b, 1, -1.0, 101.0).is_none());
    }

    #[test]
    fn pct_stop_and_target_mirror_for_shorts() {
        let mut c = ctx(&[100.0, 101.0, 104.0, 106.0], 0, 3, -1.0);
        let sl = validate(&SignalSpec { id: String::new(), rule: "sl_pct".into(), params: [("pct".into(), 3.0)].into() }).unwrap();
        let f = fire(&sl, &mut c).unwrap().unwrap();
        assert_eq!(f.idx, 2);
        // Bar 2 opens at 104, already through the 103 stop: filled at the open.
        assert_eq!((f.price, f.order), (104.0, AT_OPEN));
    }

    #[test]
    fn out_of_range_param_is_refused() {
        let bad = SignalSpec { id: "x".into(), rule: "tp_rsi".into(), params: [("level".into(), 120.0)].into() };
        assert!(validate(&bad).is_err());
        let unknown = SignalSpec { id: "x".into(), rule: "tp_rsi".into(), params: [("foo".into(), 1.0)].into() };
        assert!(validate(&unknown).is_err());
    }

    #[test]
    fn rsi_is_bounded() {
        let c: Vec<f64> = (0..50).map(|i| 100.0 + i as f64).collect();
        let r = rsi(&c, 14);
        assert_eq!(r[49], Some(100.0));
        assert!(r[13].is_none());
    }
}
