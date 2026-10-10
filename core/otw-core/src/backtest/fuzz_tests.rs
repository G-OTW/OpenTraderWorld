//! Randomized checks of the three engines (signals, grid, DCA): random markets and random
//! strategies, every run held to properties any honest backtest must satisfy.
//!
//! * **no lookahead**: the bars after a cut are replaced by another path; everything the run
//!   settled up to the cut (equity, closed trades, DCA fills) must not move;
//! * **determinism**: the same input twice gives the same result, byte for byte;
//! * **accounting**: the final equity is the capital plus the trades (and the funding), the fees
//!   reported are the fees of the trades, a DCA account's cash is its flows;
//! * **fills**: every price is one the bar traded, give or take the spread and the slippage;
//! * **costs**: the same trades with and without fees differ by exactly the fees, and those fees
//!   follow the configured fee model;
//! * **scale**: multiplying every price by a power of two (exact in floating point) changes no
//!   decision and no return, so nothing reads an absolute price level.
//!
//! Each failure prints its seed, its case and the settings, so it replays alone:
//! `OTW_FUZZ_SEED=<seed> OTW_FUZZ_CASES=<n> cargo test -p otw-core backtest::fuzz_tests`.
//! `scripts/backtest-check/run.sh` runs the long version before a release.

use super::*;
use serde_json::{json, Value};
use std::collections::HashMap;

// ── Randomness ──

/// SplitMix64: small, seedable, the same sequence on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unit()
    }
    fn int(&mut self, a: usize, b: usize) -> usize {
        a + (self.next() % (b - a + 1) as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.int(0, v.len() - 1)]
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.unit().max(1e-12), self.unit());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

fn cases() -> usize {
    std::env::var("OTW_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(40)
}

fn base_seed() -> u64 {
    std::env::var("OTW_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(20_261_004)
}

// ── Markets ──

#[derive(Clone)]
struct Market {
    ts: Vec<String>,
    o: Vec<f64>,
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
    v: Vec<f64>,
}

fn stamp(i: usize) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp(1_704_067_200 + i as i64 * 3600).unwrap();
    t.format(&time::format_description::well_known::Rfc3339).unwrap()
}

impl Market {
    /// Hourly bars from `first`: regimes of drift and volatility, overnight-style gaps, the odd
    /// large gap and the odd bar that did not trade.
    fn random(rng: &mut Rng, first: usize, n: usize, start: f64) -> Market {
        let mut m = Market { ts: Vec::new(), o: Vec::new(), h: Vec::new(), l: Vec::new(), c: Vec::new(), v: Vec::new() };
        m.extend(rng, first, n, start);
        m
    }

    fn extend(&mut self, rng: &mut Rng, first: usize, n: usize, mut prev: f64) {
        let (mut mu, mut sigma) = (0.0, 0.01);
        for i in 0..n {
            if i % 60 == 0 {
                mu = rng.range(-0.002, 0.002);
                sigma = rng.range(0.002, 0.025);
            }
            let gap = if rng.chance(0.02) { rng.range(-0.06, 0.06) } else { rng.normal() * sigma * 0.2 };
            let o = prev * (1.0 + gap);
            let (h, l, c) = if rng.chance(0.03) {
                (o, o, o)
            } else {
                let c = o * (mu + sigma * rng.normal()).exp();
                let h = o.max(c) * (1.0 + rng.unit() * sigma * 0.6);
                let l = o.min(c) * (1.0 - rng.unit() * sigma * 0.6);
                (h, l, c)
            };
            self.ts.push(stamp(first + i));
            self.o.push(o);
            self.h.push(h);
            self.l.push(l);
            self.c.push(c);
            self.v.push(1000.0 * (1.0 + rng.unit()));
            prev = c;
        }
    }

    /// The same bars up to `k`, another path after it.
    fn rewritten_after(&self, k: usize, rng: &mut Rng) -> Market {
        let mut m = Market {
            ts: Vec::new(),
            o: self.o[..=k].to_vec(),
            h: self.h[..=k].to_vec(),
            l: self.l[..=k].to_vec(),
            c: self.c[..=k].to_vec(),
            v: self.v[..=k].to_vec(),
        };
        let mut tail = Market { ts: Vec::new(), o: Vec::new(), h: Vec::new(), l: Vec::new(), c: Vec::new(), v: Vec::new() };
        let from = self.c[k] * rng.range(0.8, 1.25);
        tail.extend(rng, 0, self.ts.len() - k - 1, from);
        m.o.extend(tail.o);
        m.h.extend(tail.h);
        m.l.extend(tail.l);
        m.c.extend(tail.c);
        m.v.extend(tail.v);
        m.ts = self.ts.clone();
        m
    }

    fn scaled(&self, f: f64) -> Market {
        let s = |v: &[f64]| v.iter().map(|x| x * f).collect::<Vec<_>>();
        Market { ts: self.ts.clone(), o: s(&self.o), h: s(&self.h), l: s(&self.l), c: s(&self.c), v: self.v.clone() }
    }

    fn bars<'a>(&'a self, ticker: &'a str) -> Bars<'a> {
        Bars { ticker, ts: &self.ts, open: &self.o, high: &self.h, low: &self.l, close: &self.c, volume: &self.v, quotes: None, sub: None }
    }
}

const TICKERS: [&str; 3] = ["AAA", "BBB", "CCC"];

fn run_on(s: &Settings, markets: &[Market]) -> RunResult {
    let bars: Vec<Bars> = markets.iter().zip(TICKERS).map(|(m, t)| m.bars(t)).collect();
    let refs: Vec<&Bars> = bars.iter().collect();
    run_portfolio(s, &refs)
}

// ── Strategies ──

/// What a generated strategy must avoid for one of the comparison checks.
#[derive(Clone, Copy, Default)]
struct Limits {
    /// No absolute price anywhere (fixed fees, ticks, price trails, lot steps), and nothing
    /// decided on an equity threshold (risk breakers, Kelly, tiers).
    scale_safe: bool,
}

fn trend(rng: &mut Rng) -> Value {
    if rng.chance(0.25) {
        return json!({"kind": "price", "field": rng.pick(&["close", "open", "high", "low"])});
    }
    let ind = rng.pick(&[
        "sma", "ema", "dema", "tema", "wma", "hma", "vwap", "bb_upper", "bb_mid", "bb_lower",
        "keltner_upper", "keltner_lower", "donchian_upper", "donchian_mid", "donchian_lower", "supertrend", "psar",
    ]);
    let mult = if ind == "psar" { rng.pick(&[0.01, 0.02, 0.04]) } else { rng.pick(&[1.0, 1.5, 2.0, 3.0]) };
    json!({"kind": "indicator", "indicator": ind, "period": rng.int(2, 50), "mult": mult})
}

fn oscillator(rng: &mut Rng) -> (Value, f64) {
    let (ind, lo, hi) = rng.pick(&[
        ("rsi", 20.0, 80.0),
        ("stoch_k", 10.0, 90.0),
        ("stoch_d", 10.0, 90.0),
        ("willr", -90.0, -10.0),
        ("cci", -150.0, 150.0),
        ("mfi", 20.0, 80.0),
        ("adx", 10.0, 40.0),
    ]);
    (json!({"kind": "indicator", "indicator": ind, "period": rng.int(3, 30)}), rng.range(lo, hi).round())
}

fn condition(rng: &mut Rng) -> Value {
    let cmp = ["above", "below", "crosses_above", "crosses_below", "cross"];
    match rng.int(0, 6) {
        0 | 1 => json!({"left": trend(rng), "op": rng.pick(&cmp), "right": trend(rng)}),
        2 => {
            let (o, t) = oscillator(rng);
            json!({"left": o, "op": rng.pick(&cmp), "right": {"kind": "const", "value": t}})
        }
        3 => {
            let (f, sl) = (rng.int(3, 15), rng.int(16, 40));
            if rng.chance(0.5) {
                json!({"left": {"kind": "indicator", "indicator": "macd", "fast": f, "slow": sl},
                       "op": rng.pick(&cmp),
                       "right": {"kind": "indicator", "indicator": "macd_signal", "fast": f, "slow": sl, "signal_period": rng.int(3, 12)}})
            } else {
                json!({"left": {"kind": "indicator", "indicator": "macd_hist", "fast": f, "slow": sl},
                       "op": rng.pick(&cmp), "right": {"kind": "const", "value": 0.0}})
            }
        }
        4 => {
            let left = if rng.chance(0.5) { trend(rng) } else { oscillator(rng).0 };
            json!({"left": left, "op": rng.pick(&["rising", "falling"])})
        }
        5 => json!({"left": {"kind": "price", "field": "close"},
                    "op": rng.pick(&["closing_above", "closing_below", "opening_above", "opening_below"]),
                    "right": trend(rng)}),
        _ => {
            let (metric, lo, hi) = rng.pick(&[("change_pct", -4.0, 4.0), ("dd_from_high", 1.0, 15.0), ("up_from_low", 1.0, 15.0)]);
            json!({"left": {"kind": "metric", "metric": metric, "period": rng.int(1, 20)},
                   "op": rng.pick(&cmp), "right": {"kind": "const", "value": rng.range(lo, hi)}})
        }
    }
}

/// A small custom indicator: an oscillator around zero built from the DAG's transforms.
fn custom_condition(rng: &mut Rng) -> (Value, Value) {
    let def = json!({
        "nodes": [
            {"op": "price", "field": "close"},
            {"op": "indicator", "indicator": "ema", "period": rng.int(5, 40)},
            {"op": "sub", "a": 0, "b": 1},
            {"op": "ema_of", "a": 2, "period": rng.int(2, 10)},
            {"op": "shift", "a": 3, "n": rng.int(0, 2)},
            {"op": "div", "a": 4, "b": 1},
        ],
        "output": 5
    });
    let cond = json!({"left": {"kind": "custom_indicator", "id": "osc"}, "op": rng.pick(&["above", "below", "crosses_above", "crosses_below"]),
                      "right": {"kind": "const", "value": rng.range(-0.01, 0.01)}});
    (def, cond)
}

fn group(rng: &mut Rng, custom: &mut Option<Value>, min: usize) -> Value {
    let n = rng.int(min, 2);
    let mut conds = Vec::new();
    for _ in 0..n {
        if rng.chance(0.12) {
            let (def, c) = custom_condition(rng);
            custom.get_or_insert(def);
            conds.push(c);
        } else {
            conds.push(condition(rng));
        }
    }
    json!({"logic": rng.pick(&["all", "any"]), "conditions": conds})
}

fn stop(rng: &mut Rng, take_profit: bool) -> Value {
    if rng.chance(0.6) {
        let (a, b) = if take_profit { (0.01, 0.12) } else { (0.005, 0.08) };
        json!({"kind": "pct", "value": rng.range(a, b)})
    } else {
        json!({"kind": "atr", "value": rng.range(0.8, 4.0), "period": rng.int(5, 20)})
    }
}

fn side(rng: &mut Rng, lim: Limits, custom: &mut Option<Value>) -> Value {
    let mut s = json!({
        "entry": group(rng, custom, 1),
        "exit": if rng.chance(0.5) { group(rng, custom, 1) } else { json!({"logic": "all", "conditions": []}) },
        "exit_on_reverse": rng.chance(0.3),
    });
    if rng.chance(0.7) {
        s["stop_loss"] = stop(rng, false);
    }
    if rng.chance(0.6) {
        s["take_profit"] = stop(rng, true);
    }
    if rng.chance(0.35) {
        let abs = !lim.scale_safe && rng.chance(0.3);
        s["trailing_stop"] = json!({
            "kind": if abs { "abs" } else { "pct" },
            "value": if abs { rng.range(0.5, 5.0) } else { rng.range(0.005, 0.05) },
            "activate_pct": if rng.chance(0.5) { 0.0 } else { rng.range(0.005, 0.03) },
            "breakeven_pct": if rng.chance(0.6) { 0.0 } else { rng.range(0.005, 0.03) },
        });
    }
    s
}

fn order(rng: &mut Rng, exit: bool) -> Value {
    if rng.chance(0.6) {
        return json!({"kind": "market"});
    }
    let atr = rng.chance(0.3);
    let mut o = json!({
        "kind": "limit",
        "offset_kind": if atr { "atr" } else { "pct" },
        "offset": if atr { rng.range(0.1, 1.5) } else { rng.range(0.0, 0.01) },
        "atr_period": rng.int(5, 20),
        "reference": rng.pick(&["close", "open"]),
        "valid_bars": rng.int(1, 5),
        "fill": rng.pick(&["touch", "through"]),
    });
    if exit {
        o["on_expiry"] = json!(rng.pick(&["market", "cancel"]));
    }
    o
}

fn sizing(rng: &mut Rng, lim: Limits) -> Value {
    let pick = if lim.scale_safe { rng.int(0, 1) } else { rng.int(0, 4) };
    match pick {
        0 => json!({"mode": "percent_equity", "percent": rng.range(5.0, 90.0)}),
        1 => json!({"mode": "risk", "risk_pct": rng.range(0.3, 3.0)}),
        2 => json!({"mode": "fixed_qty", "qty": rng.range(0.5, 20.0)}),
        3 => json!({"mode": "kelly", "fraction": rng.range(0.2, 1.0), "window": rng.int(5, 20), "cap_pct": rng.range(10.0, 60.0),
                    "warmup": {"mode": "percent_equity", "percent": rng.range(2.0, 20.0)}}),
        _ => json!({"mode": "equity_tiers", "metric": rng.pick(&["qty", "percent_equity", "risk_pct"]),
                    "tiers": [{"above": 0.0, "value": rng.range(1.0, 10.0)}, {"above": 12_000.0, "value": rng.range(1.0, 10.0)}]}),
    }
}

/// A random signal strategy, as the API would receive it.
fn signals_settings(rng: &mut Rng, lim: Limits) -> Value {
    let mode = rng.pick(&["long", "short", "both"]);
    let mut custom = None;
    let mut s = json!({
        "kind": "signals",
        "mode": mode,
        "long": side(rng, lim, &mut custom),
        "short": side(rng, lim, &mut custom),
        "stop_and_reverse": rng.chance(0.25),
        "pyramiding": rng.int(1, 3),
        "pyramid_steps": {"scale": [1.0, rng.range(0.3, 1.0)], "min_distance_pct": rng.pick(&[0.0, 0.005]),
                          "after_add_sl": rng.pick(&["none", "breakeven", "trail_avg"])},
        "sizing": sizing(rng, lim),
        "starting_capital": rng.range(5_000.0, 100_000.0),
        "leverage": rng.pick(&[1.0, 1.0, 2.0, 3.0]),
        "spread_pct": rng.pick(&[0.0, 0.0005, 0.002]),
        "oos_split_pct": rng.pick(&[0.0, 0.7]),
        "execution": {
            "entry_order": order(rng, false),
            "exit_order": order(rng, true),
            "take_profit_fill": rng.pick(&["touch", "through"]),
        },
    });
    let fee_kinds: &[(&str, &str, f64)] = if lim.scale_safe {
        &[("pct", "trade", 0.1), ("pct", "unit", 0.05)]
    } else {
        &[("pct", "trade", 0.1), ("fixed", "trade", 3.0), ("fixed", "unit", 0.02), ("pct", "unit", 0.05)]
    };
    let (kind, per, top) = rng.pick(fee_kinds);
    s["fees"] = json!({"amount_kind": kind, "per": per, "amount": rng.range(0.0, top)});
    s["slippage"] = if !lim.scale_safe && rng.chance(0.2) {
        json!({"kind": "ticks", "value": rng.int(1, 3) as f64, "tick_size": 0.01})
    } else {
        json!({"kind": "pct", "value": rng.pick(&[0.0, 0.0002, 0.001])})
    };
    if rng.chance(0.3) {
        s["execution"]["maker_fee"] = json!(rng.range(0.0, 0.05));
    }
    if rng.chance(0.25) {
        s["funding"] = json!({"annual_rate_pct": rng.range(-10.0, 20.0), "interval_hours": 8.0});
    }
    if !lim.scale_safe && rng.chance(0.3) {
        s["risk"] = json!({
            "max_drawdown_pct": rng.pick(&[10.0, 25.0]),
            "max_daily_loss_pct": rng.pick(&[2.0, 5.0]),
            "max_open_positions": rng.int(1, 2),
            "max_exposure_pct": rng.pick(&[80.0, 150.0, 300.0]),
        });
    }
    if !lim.scale_safe && rng.chance(0.2) {
        s["instrument"] = json!({"multiplier": rng.pick(&[1.0, 2.0, 10.0]), "lot_step": rng.pick(&[0.0, 0.01, 1.0]), "min_qty": rng.pick(&[0.0, 1.0])});
    }
    if rng.chance(0.25) {
        s["filters"] = json!({
            "weekdays": if rng.chance(0.5) { json!([1, 2, 3, 4, 5]) } else { json!([]) },
            "sessions": if rng.chance(0.6) { json!([{"from": "08:00", "to": "20:00"}]) } else { json!([]) },
            "timezone": rng.pick(&["", "Europe/Paris", "America/New_York"]),
            "tz_offset_min": rng.pick(&[0, 120, -300]),
            "on_window_end": rng.pick(&["hold", "flat"]),
            "block_adds": rng.chance(0.5),
        });
    }
    if let Some(def) = custom {
        s["indicators"] = json!({"osc": def});
    }
    s
}

fn grid_settings(rng: &mut Rng, start: f64) -> Value {
    let anchored = rng.chance(0.5);
    let banded = !anchored && rng.chance(0.7);
    let (lo, hi) = if banded { (start * rng.range(0.6, 0.95), start * rng.range(1.05, 1.5)) } else { (0.0, 0.0) };
    json!({
        "kind": "grid",
        "mode": "long",
        "sizing": {"mode": "percent_equity", "percent": 10.0},
        "starting_capital": 10_000.0,
        "spread_pct": rng.pick(&[0.0, 0.001]),
        "fees": {"amount_kind": "pct", "per": "trade", "amount": rng.range(0.0, 0.1)},
        "grid": {
            "lower": lo, "upper": hi,
            "levels": rng.int(3, 20),
            "qty_per_level": if rng.chance(0.5) { 0.0 } else { rng.range(0.5, 5.0) },
            "total_budget": if rng.chance(0.5) { 0.0 } else { rng.range(1_000.0, 10_000.0) },
            "direction": rng.pick(&["long", "short"]),
            "stop_below": if banded && rng.chance(0.3) { lo * 0.95 } else { 0.0 },
            "stop_above": if banded && rng.chance(0.3) { hi * 1.05 } else { 0.0 },
            "anchor": if anchored { rng.pick(&["sma", "ema", "wma", "hma", "vwap"]) } else { "none" },
            "anchor_period": rng.int(5, 50),
            "width_kind": rng.pick(&["pct", "atr"]),
            "width_value": rng.range(1.0, 6.0),
            "width_period": rng.int(5, 30),
            "reset_on_close": rng.chance(0.15),
        },
    })
}

fn dca_settings(rng: &mut Rng, n_assets: usize) -> Value {
    let weights: Vec<Value> = TICKERS[..n_assets].iter().map(|t| json!({"ticker": t, "weight": rng.range(0.1, 1.0)})).collect();
    let pos_cond = || json!({"logic": "all", "conditions": []});
    let mut buys = Vec::new();
    for _ in 0..rng.int(0, 2) {
        let cond = if rng.chance(0.4) {
            json!({"logic": "all", "conditions": [{"left": {"kind": "position", "field": rng.pick(&["pnl_pct", "since_last_buy_pct", "drawdown_pct"])},
                                                    "op": rng.pick(&["below", "above"]), "right": {"kind": "const", "value": rng.range(-10.0, 10.0)}}]})
        } else {
            json!({"logic": "all", "conditions": [condition(rng)]})
        };
        // A percentage above 100 paid in as new money compounds every fire (x3 an hour runs past
        // f64 in a few hundred bars): the plan stays within what an account could fund.
        let kind = rng.pick(&["fixed", "pct_cash", "pct_equity", "pct_invested"]);
        buys.push(json!({
            "amount_kind": kind,
            "amount": if kind == "fixed" { rng.range(10.0, 300.0) } else { rng.range(1.0, 100.0) },
            "per_asset": rng.chance(0.5),
            "condition": cond,
            "max_fires": rng.int(0, 5),
            "cooldown_bars": rng.int(0, 48),
        }));
    }
    let mut sells = Vec::new();
    for _ in 0..rng.int(0, 2) {
        sells.push(json!({
            "amount_kind": rng.pick(&["pct_position", "all", "units", "amount"]),
            "amount": rng.range(1.0, 50.0),
            "per_asset": rng.chance(0.5),
            "target_gain_pct": if rng.chance(0.5) { rng.range(2.0, 20.0) } else { 0.0 },
            "condition": if rng.chance(0.5) { json!({"logic": "all", "conditions": [condition(rng)]}) } else { pos_cond() },
            "cooldown_bars": rng.int(0, 24),
            "withdraw": rng.chance(0.3),
        }));
    }
    json!({
        "kind": "dca",
        "mode": "long",
        "sizing": {"mode": "percent_equity", "percent": 10.0},
        "starting_capital": rng.pick(&[0.0, 1_000.0, 10_000.0]),
        "spread_pct": rng.pick(&[0.0, 0.001]),
        "slippage": {"kind": "pct", "value": rng.pick(&[0.0, 0.0005])},
        "fees": {"amount_kind": rng.pick(&["pct", "fixed"]), "per": "trade", "amount": rng.range(0.0, 1.0)},
        "dca": {
            "weights": weights,
            "contribution": {"amount": rng.range(50.0, 500.0), "period": rng.pick(&["day", "week", "month"]), "every": rng.int(1, 2), "invest": rng.chance(0.8)},
            "buys": buys,
            "sells": sells,
        },
    })
}

/// Parse and validate generated settings. None for a combination the API refuses.
fn settle(v: &Value) -> Option<Settings> {
    let s: Settings = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("generated settings do not parse: {e}\n{v}"));
    s.validate().is_none().then_some(s)
}

// ── Properties ──

struct Ctx<'a> {
    label: String,
    settings: &'a Value,
}

impl Ctx<'_> {
    fn fail(&self, what: String) -> ! {
        panic!("{}: {what}\nsettings: {}", self.label, self.settings);
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1.0)
}

fn deterministic(cx: &Ctx, s: &Settings, markets: &[Market], first: &RunResult) {
    let again = run_on(s, markets);
    if serde_json::to_string(first).unwrap() != serde_json::to_string(&again).unwrap() {
        cx.fail("two runs of the same input differ".into());
    }
}

fn finite(cx: &Ctx, r: &RunResult) {
    if let Some(e) = r.equity.iter().find(|e| !e.equity.is_finite()) {
        cx.fail(format!("equity is not finite at {}", e.ts));
    }
    for t in r.trades.iter() {
        if ![t.entry_price, t.exit_price, t.qty, t.pnl, t.fees].iter().all(|x| x.is_finite()) {
            cx.fail(format!("trade with a non-finite field: {t:?}"));
        }
    }
}

/// Everything settled up to bar `k` of every asset is the same when the bars after `k` change.
fn no_lookahead(cx: &Ctx, s: &Settings, markets: &[Market], full: &RunResult, rng: &mut Rng) {
    let n = markets[0].ts.len();
    let k = rng.int(n / 3, n - 3);
    let cut = markets[0].ts[k].clone();
    let other: Vec<Market> = markets.iter().map(|m| {
        let kk = m.ts.iter().position(|t| *t == cut).unwrap_or(m.ts.len() - 1);
        m.rewritten_after(kk, rng)
    }).collect();
    let alt = run_on(s, &other);
    let upto = |r: &RunResult| -> Vec<String> {
        r.equity.iter().take_while(|e| e.ts <= cut).map(|e| format!("{} {:.10e}", e.ts, e.equity)).collect()
    };
    let (a, b) = (upto(full), upto(&alt));
    if a != b {
        let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        cx.fail(format!("equity up to {cut} moved with the bars after it, first at row {at}: {:?} vs {:?}", a.get(at), b.get(at)));
    }
    let settled = |r: &RunResult| -> Vec<String> {
        r.trades.iter().filter(|t| t.exit_ts <= cut).map(|t| serde_json::to_string(t).unwrap()).collect()
    };
    let (ta, tb) = (settled(full), settled(&alt));
    if ta != tb {
        let at = ta.iter().zip(&tb).position(|(x, y)| x != y).unwrap_or(ta.len().min(tb.len()));
        cx.fail(format!("trades closed by {cut} moved with the bars after it: {:?} vs {:?}", ta.get(at), tb.get(at)));
    }
    if let (Some(d0), Some(d1)) = (&full.dca, &alt.dca) {
        let fills = |d: &dca::DcaStats| -> Vec<String> {
            d.events.iter().filter(|e| e.ts <= cut).map(|e| format!("{} {} {} {:.10e} {:.10e}", e.ts, e.ticker, e.action, e.price, e.qty)).collect()
        };
        if d0.events_total <= d0.events.len() && fills(d0) != fills(d1) {
            cx.fail(format!("DCA fills up to {cut} moved with the bars after it"));
        }
    }
}

/// The signal engine's books: capital + trades + funding is the final equity, and the fees.
fn accounting(cx: &Ctx, s: &Settings, r: &RunResult) {
    let pnl: f64 = r.trades.iter().map(|t| t.pnl).sum();
    let want = s.starting_capital + pnl + r.total_funding;
    if !close(r.stats.final_equity, want, 1e-9) {
        cx.fail(format!("final equity {} != capital + trades + funding {}", r.stats.final_equity, want));
    }
    if let Some(last) = r.equity.last() {
        if !close(last.equity, r.stats.final_equity, 1e-9) {
            cx.fail(format!("last equity point {} != final equity {}", last.equity, r.stats.final_equity));
        }
    }
    let fees: f64 = r.trades.iter().map(|t| t.fees).sum();
    if !close(fees, r.stats.total_fees, 1e-9) || r.trades.iter().any(|t| t.fees < -1e-12) {
        cx.fail(format!("fees reported {} != fees of the trades {}", r.stats.total_fees, fees));
    }
}

/// Every price is one the bars traded, widened by the spread and the slippage.
fn fills_in_range(cx: &Ctx, s: &Settings, markets: &[Market], r: &RunResult) {
    let idx: HashMap<(&str, &str), usize> = markets
        .iter()
        .zip(TICKERS)
        .flat_map(|(m, t)| m.ts.iter().enumerate().map(move |(i, ts)| ((t, ts.as_str()), i)))
        .collect();
    let by: HashMap<&str, &Market> = markets.iter().zip(TICKERS).map(|(m, t)| (t, m)).collect();
    let hs = s.spread_pct / 2.0;
    let slip = if s.slippage.kind == "ticks" { 0.0 } else { s.slippage.value };
    let ticks = if s.slippage.kind == "ticks" { s.slippage.value * s.slippage.tick_size } else { 0.0 };
    let span = |m: &Market, a: usize, b: usize| {
        let lo = m.l[a..=b].iter().copied().fold(f64::INFINITY, f64::min);
        let hi = m.h[a..=b].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo * (1.0 - hs) * (1.0 - slip) - ticks - 1e-9, hi * (1.0 + hs) * (1.0 + slip) + ticks + 1e-9)
    };
    for t in &r.trades {
        let m = by[t.ticker.as_str()];
        let (Some(&e), Some(&x)) = (idx.get(&(t.ticker.as_str(), t.entry_ts.as_str())), idx.get(&(t.ticker.as_str(), t.exit_ts.as_str()))) else {
            cx.fail(format!("trade stamped off its asset's bars: {t:?}"));
        };
        if x < e || t.qty <= 0.0 || t.entries == 0 || t.bars_held != x - e {
            cx.fail(format!("trade out of order or empty: {t:?}"));
        }
        let (lo, hi) = span(m, e, x);
        if t.entry_price < lo || t.entry_price > hi {
            cx.fail(format!("entry price {} outside the bars it was built on [{lo}, {hi}]: {t:?}", t.entry_price));
        }
        let (lo, hi) = span(m, x, x);
        if t.exit_price < lo || t.exit_price > hi {
            cx.fail(format!("exit price {} outside its bar [{lo}, {hi}]: {t:?}", t.exit_price));
        }
    }
}

// ── The runs ──

#[test]
fn random_signal_strategies_hold_every_property() {
    let seed = base_seed();
    let (mut ran, mut trades) = (0usize, 0usize);
    for case in 0..cases() {
        let mut rng = Rng(seed ^ (case as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let v = signals_settings(&mut rng, Limits::default());
        let Some(s) = settle(&v) else { continue };
        let n_assets = if rng.chance(0.25) { rng.int(2, 3) } else { 1 };
        let n = rng.int(200, 500);
        let markets: Vec<Market> = (0..n_assets)
            .map(|a| {
                let first = if a == 0 { 0 } else { rng.int(0, 30) };
                {
                    let p = rng.range(20.0, 400.0);
                    Market::random(&mut rng, first, n - first, p)
                }
            })
            .collect();
        let cx = Ctx { label: format!("signals seed {seed} case {case}"), settings: &v };
        let r = run_on(&s, &markets);
        finite(&cx, &r);
        accounting(&cx, &s, &r);
        fills_in_range(&cx, &s, &markets, &r);
        deterministic(&cx, &s, &markets, &r);
        no_lookahead(&cx, &s, &markets, &r, &mut rng);
        ran += 1;
        trades += r.trades.len();
    }
    eprintln!("signals: {ran} strategies, {trades} trades");
    assert!(ran * 2 >= cases(), "too many generated strategies refused: {ran}/{}", cases());
    assert!(trades > ran, "the generated strategies hardly trade: {trades} trades over {ran} runs");
}

/// Prices x4 and x1/4 (exact in binary floating point): same decisions, same returns.
#[test]
fn random_strategies_ignore_the_price_level() {
    let seed = base_seed().wrapping_add(1);
    let mut compared = 0usize;
    for case in 0..cases() {
        let mut rng = Rng(seed ^ (case as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let v = signals_settings(&mut rng, Limits { scale_safe: true });
        let Some(s) = settle(&v) else { continue };
        let (len, p) = (rng.int(200, 400), rng.range(20.0, 400.0));
        let m = Market::random(&mut rng, 0, len, p);
        let base = run_on(&s, std::slice::from_ref(&m));
        let cx = Ctx { label: format!("scale seed {seed} case {case}"), settings: &v };
        for f in [4.0, 0.25] {
            let r = run_on(&s, &[m.scaled(f)]);
            if r.trades.len() != base.trades.len() {
                cx.fail(format!("x{f}: {} trades instead of {}", r.trades.len(), base.trades.len()));
            }
            for (a, b) in base.trades.iter().zip(&r.trades) {
                let same = a.entry_ts == b.entry_ts && a.exit_ts == b.exit_ts && a.exit_reason == b.exit_reason && a.entries == b.entries;
                if !same || !close(a.entry_price * f, b.entry_price, 1e-9) || !close(a.return_pct, b.return_pct, 1e-6) {
                    cx.fail(format!("x{f}: a trade changed with the price level\n{a:?}\n{b:?}"));
                }
            }
            if !close(base.stats.return_pct, r.stats.return_pct, 1e-6) {
                cx.fail(format!("x{f}: return {} instead of {}", r.stats.return_pct, base.stats.return_pct));
            }
        }
        compared += base.trades.len();
    }
    eprintln!("scale: {compared} trades compared");
}

/// The same trades with and without fees: the P&L differs by the fees, and the fees are the
/// configured model applied to the trade's own quantity and prices.
#[test]
fn fees_cost_exactly_the_fee_model() {
    let seed = base_seed().wrapping_add(2);
    let mut checked = 0usize;
    for case in 0..cases() {
        let mut rng = Rng(seed ^ (case as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let mut v = signals_settings(&mut rng, Limits::default());
        // Trades must not depend on the equity the fees take away: a fixed quantity, a deep
        // account, no breaker, and fills that do not move with the costs.
        v["sizing"] = json!({"mode": "fixed_qty", "qty": rng.range(0.5, 5.0)});
        v["starting_capital"] = json!(1.0e9);
        v["risk"] = json!({});
        v["spread_pct"] = json!(0.0);
        v["slippage"] = json!({"kind": "pct", "value": 0.0});
        v["funding"] = json!({"annual_rate_pct": 0.0});
        v["execution"]["maker_fee"] = Value::Null;
        let (kind, per) = rng.pick(&[("pct", "trade"), ("fixed", "trade"), ("fixed", "unit"), ("pct", "unit")]);
        let amount = rng.range(0.01, 2.0);
        v["fees"] = json!({"amount_kind": kind, "per": per, "amount": amount});
        let Some(s) = settle(&v) else { continue };
        let mut free = v.clone();
        free["fees"]["amount"] = json!(0.0);
        let s0 = settle(&free).unwrap();
        let (len, p) = (rng.int(200, 400), rng.range(20.0, 400.0));
        let m = Market::random(&mut rng, 0, len, p);
        let cx = Ctx { label: format!("fees seed {seed} case {case}"), settings: &v };
        let (with, without) = (run_on(&s, std::slice::from_ref(&m)), run_on(&s0, std::slice::from_ref(&m)));
        if with.trades.len() != without.trades.len() {
            cx.fail(format!("fees changed the trades: {} vs {}", with.trades.len(), without.trades.len()));
        }
        let mult = s.instrument.multiplier.max(1e-12);
        for (a, b) in with.trades.iter().zip(&without.trades) {
            if a.entry_ts != b.entry_ts || a.exit_ts != b.exit_ts || !close(a.qty, b.qty, 1e-12) || b.fees != 0.0 {
                cx.fail(format!("fees changed a trade\n{a:?}\n{b:?}"));
            }
            if !close(b.pnl - a.pnl, a.fees, 1e-9) {
                cx.fail(format!("P&L moved by {} for {} of fees: {a:?}", b.pnl - a.pnl, a.fees));
            }
            let lots = a.entries as f64;
            let want = match (kind, per) {
                ("pct", _) => amount / 100.0 * a.qty * mult * (a.entry_price + a.exit_price),
                ("fixed", "trade") => amount * (lots + 1.0),
                _ => amount * a.qty * 2.0,
            };
            if !close(a.fees, want, 1e-9) {
                cx.fail(format!("{kind}/{per} fee {amount}: charged {} instead of {want}: {a:?}", a.fees));
            }
        }
        checked += with.trades.len();
    }
    eprintln!("fees: {checked} trades checked");
}

#[test]
fn random_grids_hold_their_properties() {
    let seed = base_seed().wrapping_add(3);
    for case in 0..cases() {
        let mut rng = Rng(seed ^ (case as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let start = rng.range(20.0, 400.0);
        let v = grid_settings(&mut rng, start);
        let Some(s) = settle(&v) else { continue };
        let len = rng.int(200, 500);
        let m = Market::random(&mut rng, 0, len, start);
        let markets = [m];
        let cx = Ctx { label: format!("grid seed {seed} case {case}"), settings: &v };
        let r = run_on(&s, &markets);
        finite(&cx, &r);
        fills_in_range(&cx, &s, &markets, &r);
        deterministic(&cx, &s, &markets, &r);
        no_lookahead(&cx, &s, &markets, &r, &mut rng);
    }
}

#[test]
fn random_dca_plans_hold_their_properties() {
    let seed = base_seed().wrapping_add(4);
    for case in 0..cases() {
        let mut rng = Rng(seed ^ (case as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let n_assets = rng.int(1, 3);
        let v = dca_settings(&mut rng, n_assets);
        let Some(s) = settle(&v) else { continue };
        let n = rng.int(300, 900);
        let markets: Vec<Market> = (0..n_assets)
            .map(|a| {
                let first = if a == 0 { 0 } else { rng.int(0, 60) };
                {
                    let p = rng.range(20.0, 400.0);
                    Market::random(&mut rng, first, n - first, p)
                }
            })
            .collect();
        let cx = Ctx { label: format!("dca seed {seed} case {case}"), settings: &v };
        let r = run_on(&s, &markets);
        finite(&cx, &r);
        deterministic(&cx, &s, &markets, &r);
        no_lookahead(&cx, &s, &markets, &r, &mut rng);
        let d = r.dca.as_ref().unwrap_or_else(|| cx.fail("a DCA run without its DCA block".into()));
        // Buys never spend money the account does not have; only a fee larger than what a
        // sell brings in can leave the cash below zero, as a broker would debit it.
        if d.cash < -d.fees - 1e-6 {
            cx.fail(format!("the plan spent money it did not have: cash {}", d.cash));
        }
        // Cash is the money in plus every fill: buys out, sells in unless withdrawn.
        if d.events_total == d.events.len() {
            let flows: f64 = d.events.iter().map(|e| e.amount).sum();
            let want = d.contributed + flows - d.withdrawn;
            // Rounding accumulates over every flow, so the tolerance follows their volume.
            let volume: f64 = d.contributed + d.withdrawn + d.events.iter().map(|e| e.amount.abs()).sum::<f64>();
            if (d.cash - want).abs() > 1e-9 * volume.max(1.0) {
                cx.fail(format!("cash {} != contributions + fills - withdrawals {want}", d.cash));
            }
        }
        // Fills at an open, within the bar give or take the spread and the slippage.
        let hs = s.spread_pct / 2.0;
        let slip = s.slippage.value;
        for e in &d.events {
            let ai = TICKERS.iter().position(|t| *t == e.ticker).unwrap();
            let m = &markets[ai];
            let Some(i) = m.ts.iter().position(|t| *t == e.ts) else { cx.fail(format!("fill off its asset's bars: {}", e.ts)) };
            let (lo, hi) = (m.l[i] * (1.0 - hs) * (1.0 - slip) - 1e-9, m.h[i] * (1.0 + hs) * (1.0 + slip) + 1e-9);
            if e.price < lo || e.price > hi {
                cx.fail(format!("{} at {} outside its bar [{lo}, {hi}] on {}", e.action, e.price, e.ts));
            }
        }
    }
}
