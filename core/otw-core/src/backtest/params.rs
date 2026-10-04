//! Which parameters of a strategy an optimizer axis can vary, read off the settings itself.
//!
//! The UI answers this question in `frontend/src/lib/modules/backtest/optimize.js`
//! (`discoverParams`), which is fine for a screen: the picker knows the indicator catalog, the
//! i18n labels and the display units the user typed in. A remote caller has none of that, so an
//! axis path is a guess, and a wrong path is silently written into the settings as a new key
//! that the engine never reads. The grid then runs to completion and reports the base strategy N
//! times over.
//!
//! So this walks the settings a caller is about to optimize and reports the paths that exist,
//! with the value each one holds today. Two deliberate differences from the UI version:
//!
//! - **Engine units, never display units.** The UI carries a `scale` because a user types "2%"
//!   where the engine stores `0.02`. A grid value is written into the settings verbatim, so
//!   everything here (value and suggested range alike) is already what the engine reads, and
//!   `unit` only says how to *read* that number back to a human.
//! - **What is set, not what could be.** A parameter is offered when the strategy actually uses
//!   it (the sizing mode in force, a stop that exists, an indicator param that is non-zero).
//!   Sweeping a field the engine ignores is the same dead grid as a misspelled path.
//!
//! No indicator catalog is duplicated here: an indicator operand carries its own params, and a
//! param it does not use is left at zero by every writer of these settings.

use serde::Serialize;
use serde_json::Value;

/// One varyable parameter of a strategy.
#[derive(Debug, Clone, Serialize)]
pub struct Param {
    /// Settings paths this parameter writes. More than one when the same number has to be
    /// written in two places to stay coherent (a percent stop lives in the rule object and in
    /// the legacy mirror field), which is exactly what `Axis.paths` is for.
    pub paths: Vec<String>,
    /// Section the UI groups it under: signals | risk | grid | sizing | costs | limits.
    pub group: &'static str,
    /// Human label, already carrying where the parameter sits.
    pub label: String,
    /// The value the strategy holds today, in engine units.
    pub value: f64,
    /// "int" (whole numbers only) or "num".
    pub kind: &'static str,
    /// How to read `value`: "fraction" (0.02 = 2%), "percent" (2 = 2%), "multiple", "ticks",
    /// "bars" or "" when the number is bare.
    pub unit: &'static str,
    /// Suggested sweep, in engine units: half to one and a half times the current value in
    /// about nine steps. It is a starting point, not a recommendation.
    pub suggested: Range,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Range {
    pub from: f64,
    pub to: f64,
    pub step: f64,
}

/// Round off the float noise a division leaves behind (0.30000000000000004).
fn tidy(v: f64) -> f64 {
    let r = format!("{v:.6}").parse::<f64>().unwrap_or(v);
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

fn int_range(value: f64, min: f64) -> Range {
    let v = value.round().max(min);
    let from = (v * 0.5).round().max(min);
    let to = (v * 1.5).round().max(from + 1.0);
    Range { from, to, step: (((to - from) / 9.0).round()).max(1.0) }
}

fn num_range(value: f64, min: f64) -> Range {
    let base = if value != 0.0 { value.abs() } else { 1.0 };
    let from = tidy((base * 0.5).max(min));
    let to = tidy(base * 1.5);
    let step = tidy(((to - from) / 8.0).max(base / 100.0));
    Range { from, to, step: if step > 0.0 { step } else { 0.1 } }
}

struct Builder(Vec<Param>);

impl Builder {
    fn push(
        &mut self,
        paths: &[String],
        group: &'static str,
        label: String,
        value: f64,
        kind: &'static str,
        unit: &'static str,
        min: f64,
    ) {
        // A mirrored strategy reaches the same path twice (its short side is the long one
        // written out); the first spelling wins so the caller is not offered a duplicate axis.
        if self.0.iter().any(|p| p.paths == paths) {
            return;
        }
        let suggested = if kind == "int" { int_range(value, min) } else { num_range(value, min) };
        self.0.push(Param {
            paths: paths.to_vec(),
            group,
            label,
            value: if kind == "int" { value.round() } else { tidy(value) },
            kind,
            unit,
            suggested,
        });
    }

    fn int(&mut self, path: String, group: &'static str, label: String, value: f64, min: f64) {
        self.push(&[path], group, label, value, "int", "", min);
    }

    fn num(
        &mut self,
        path: String,
        group: &'static str,
        label: String,
        value: f64,
        unit: &'static str,
        min: f64,
    ) {
        self.push(&[path], group, label, value, "num", unit, min);
    }
}

fn f(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Every parameter of `settings` a grid can vary, in the order a picker would show them.
pub fn discover(settings: &Value) -> Vec<Param> {
    let mut b = Builder(Vec::new());
    let mode = s(settings, "mode");
    let kind = s(settings, "kind");
    let is_grid = kind == "grid";
    let is_dca = kind == "dca";

    if !is_grid && !is_dca {
        for side in ["long", "short"] {
            if (side == "long" && mode == "short") || (side == "short" && mode == "long") {
                continue;
            }
            let Some(cfg) = settings.get(side) else { continue };
            for group in ["entry", "exit"] {
                signal_params(&mut b, cfg, side, group);
            }
            stop_params(&mut b, cfg, side);
        }
    }
    grid_params(&mut b, settings, is_grid);
    dca_params(&mut b, settings, is_dca);
    money_params(&mut b, settings, is_dca);
    b.0
}

/// The parameters of a savings plan, for a DCA strategy only: what is paid in, what a tranche
/// deploys and when a rule may fire again. Without these the picker offered a DCA plan nothing
/// but sizing knobs the mode ignores, so a grid over them ran the same simulation N times.
fn dca_params(b: &mut Builder, settings: &Value, is_dca: bool) {
    if !is_dca {
        return;
    }
    let Some(d) = settings.get("dca") else { return };
    if let Some(c) = d.get("contribution").filter(|v| v.is_object()) {
        if f(c, "amount") > 0.0 {
            b.num("dca.contribution.amount".into(), "dca", "contribution".into(), f(c, "amount"), "", 0.0);
        }
        let every = f(c, "every");
        if every >= 1.0 {
            b.int("dca.contribution.every".into(), "dca", "contribute every".into(), every, 1.0);
        }
    }
    let name = |r: &Value, fallback: String| {
        let n = s(r, "name").trim().to_string();
        if n.is_empty() { fallback } else { n }
    };
    for (i, r) in d.get("buys").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let who = name(r, format!("buy {}", i + 1));
        let unit = if s(r, "amount_kind") == "fixed" { "" } else { "percent" };
        if f(r, "amount") > 0.0 {
            b.num(format!("dca.buys.{i}.amount"), "dca", format!("{who} amount"), f(r, "amount"), unit, 0.0);
        }
        rule_gates(b, r, i, "buys", &who);
        threshold_params(b, r.get("condition"), &format!("dca.buys.{i}.condition"), &who);
    }
    for (i, r) in d.get("sells").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let who = name(r, format!("sell {}", i + 1));
        if f(r, "amount") > 0.0 && s(r, "amount_kind") != "all" {
            let unit = if s(r, "amount_kind") == "pct_position" { "percent" } else { "" };
            b.num(format!("dca.sells.{i}.amount"), "dca", format!("{who} amount"), f(r, "amount"), unit, 0.0);
        }
        if f(r, "target_gain_pct") > 0.0 {
            b.num(
                format!("dca.sells.{i}.target_gain_pct"),
                "dca",
                format!("{who} objective"),
                f(r, "target_gain_pct"),
                "percent",
                0.0,
            );
        }
        rule_gates(b, r, i, "sells", &who);
        threshold_params(b, r.get("condition"), &format!("dca.sells.{i}.condition"), &who);
    }
}

/// `max_fires` / `cooldown_bars` of one plan rule, offered only when the rule sets them (a 0
/// means "no cap", and sweeping a cap onto a rule that has none changes what the rule is).
fn rule_gates(b: &mut Builder, r: &Value, i: usize, list: &str, who: &str) {
    for (key, label) in [("max_fires", "max fires"), ("cooldown_bars", "cooldown")] {
        let v = f(r, key);
        if v > 0.0 {
            b.int(format!("dca.{list}.{i}.{key}"), "dca", format!("{who} {label}"), v, 1.0);
        }
    }
}

/// Indicator lengths and constant thresholds of a plan rule's condition group. Same shapes as
/// a side's signal group, addressed at the rule's own path.
fn threshold_params(b: &mut Builder, group: Option<&Value>, base: &str, who: &str) {
    let conds = group
        .and_then(|g| g.get("conditions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, cond) in conds.iter().enumerate() {
        for slot in ["left", "right"] {
            let Some(o) = cond.get(slot) else { continue };
            let path = format!("{base}.conditions.{i}.{slot}");
            match s(o, "kind") {
                "const" => b.num(
                    format!("{path}.value"),
                    "dca",
                    format!("{who} threshold {}", i + 1),
                    f(o, "value"),
                    "",
                    0.0,
                ),
                "metric" if f(o, "period") > 0.0 => b.int(
                    format!("{path}.period"),
                    "dca",
                    format!("{who} {} lookback", s(o, "metric")),
                    f(o, "period"),
                    1.0,
                ),
                "indicator" => {
                    for (key, label) in [("period", "period"), ("fast", "fast length"), ("slow", "slow length")] {
                        let v = f(o, key);
                        if v > 0.0 {
                            b.int(
                                format!("{path}.{key}"),
                                "dca",
                                format!("{who} {} {label}", s(o, "indicator").to_uppercase()),
                                v,
                                1.0,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Indicator and threshold parameters of one signal group.
fn signal_params(b: &mut Builder, side_cfg: &Value, side: &str, group: &str) {
    let conds = side_cfg
        .get(group)
        .and_then(|g| g.get("conditions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, cond) in conds.iter().enumerate() {
        for slot in ["left", "right"] {
            let Some(o) = cond.get(slot) else { continue };
            let base = format!("{side}.{group}.conditions.{i}.{slot}");
            let where_ = format!("{side} {group} {}", i + 1);
            match s(o, "kind") {
                "indicator" => {
                    let name = s(o, "indicator").to_uppercase();
                    for (key, label, kind) in [
                        ("period", "period", "int"),
                        ("fast", "fast length", "int"),
                        ("slow", "slow length", "int"),
                        ("signal_period", "signal length", "int"),
                        ("mult", "multiplier", "num"),
                    ] {
                        // Zero is how an operand says "this indicator has no such param".
                        let v = f(o, key);
                        if v <= 0.0 {
                            continue;
                        }
                        let path = format!("{base}.{key}");
                        let text = format!("{name} {label} ({where_})");
                        if kind == "int" {
                            b.int(path, "signals", text, v, 1.0);
                        } else {
                            b.num(path, "signals", text, v, "multiple", 0.0);
                        }
                    }
                }
                // A custom indicator carries a definition, not params: nothing to vary from here.
                "const" => b.num(
                    format!("{base}.value"),
                    "signals",
                    format!("threshold ({where_})"),
                    f(o, "value"),
                    "",
                    f64::NEG_INFINITY,
                ),
                _ => {}
            }
        }
    }
}

/// Stop-loss and take-profit of one side, in whichever of the three forms it is configured.
fn stop_params(b: &mut Builder, side_cfg: &Value, side: &str) {
    for (key, label) in [("stop_loss", "stop loss"), ("take_profit", "take profit")] {
        let rule = side_cfg.get(key);
        let legacy = f(side_cfg, &format!("{key}_pct"));
        match rule {
            Some(r) if s(r, "kind") == "atr" => {
                b.num(
                    format!("{side}.{key}.value"),
                    "risk",
                    format!("{side} {label} (ATR multiple)"),
                    f(r, "value"),
                    "multiple",
                    0.0,
                );
                let period = f(r, "period");
                b.int(
                    format!("{side}.{key}.period"),
                    "risk",
                    format!("{side} {label} ATR length"),
                    if period > 0.0 { period } else { 14.0 },
                    1.0,
                );
            }
            Some(r) if f(r, "value") > 0.0 => {
                // The rule object and the legacy field must not disagree, so one value writes
                // both: the engine reads the object, an older reader reads the mirror.
                b.push(
                    &[format!("{side}.{key}.value"), format!("{side}.{key}_pct")],
                    "risk",
                    format!("{side} {label}"),
                    f(r, "value"),
                    "num",
                    "fraction",
                    0.0,
                );
            }
            _ if legacy > 0.0 => b.num(
                format!("{side}.{key}_pct"),
                "risk",
                format!("{side} {label}"),
                legacy,
                "fraction",
                0.0,
            ),
            _ => {}
        }
    }
    trail_params(b, side_cfg, side);
}

/// Trailing-stop axes of one side: the distance in its own unit, plus the two thresholds only
/// when the config actually uses them (an axis pinned at 0 varies nothing).
fn trail_params(b: &mut Builder, side_cfg: &Value, side: &str) {
    let Some(t) = side_cfg.get("trailing_stop").filter(|v| v.is_object()) else { return };
    let value = f(t, "value");
    let kind = s(t, "kind");
    // A distance of 0 with a breakeven step is a legal plan: the distance axis is dead there,
    // the breakeven one is not.
    if value > 0.0 {
        let (unit, label) = match kind {
            "abs" => ("", "trailing stop (price distance)"),
            _ => ("fraction", "trailing stop"),
        };
        b.num(format!("{side}.trailing_stop.value"), "risk", format!("{side} {label}"), value, unit, 0.0);
    }
    for (key, label) in
        [("activate_pct", "trailing stop activation"), ("breakeven_pct", "trailing stop breakeven")]
    {
        let v = f(t, key);
        if v > 0.0 {
            b.num(
                format!("{side}.trailing_stop.{key}"),
                "risk",
                format!("{side} {label}"),
                v,
                "fraction",
                0.0,
            );
        }
    }
}

/// Ladder parameters, for a grid strategy only.
fn grid_params(b: &mut Builder, settings: &Value, is_grid: bool) {
    if !is_grid {
        return;
    }
    let Some(g) = settings.get("grid") else { return };
    let levels = f(g, "levels");
    b.int("grid.levels".into(), "grid", "grid levels".into(), if levels > 0.0 { levels } else { 10.0 }, 2.0);
    let anchor = s(g, "anchor");
    if !anchor.is_empty() && anchor != "none" {
        let p = f(g, "anchor_period");
        b.int("grid.anchor_period".into(), "grid", "anchor length".into(), if p > 0.0 { p } else { 20.0 }, 1.0);
        let unit = if s(g, "width_kind") == "atr" { "multiple" } else { "percent" };
        b.num("grid.width_value".into(), "grid", "band half-width".into(), f(g, "width_value"), unit, 0.0);
        if s(g, "width_kind") == "atr" {
            let wp = f(g, "width_period");
            b.int("grid.width_period".into(), "grid", "band ATR length".into(), if wp > 0.0 { wp } else { 14.0 }, 1.0);
        }
    } else {
        for (key, label) in [("lower", "grid lower bound"), ("upper", "grid upper bound")] {
            if f(g, key) > 0.0 {
                b.num(format!("grid.{key}"), "grid", label.into(), f(g, key), "", 0.0);
            }
        }
    }
    for (key, label) in [("qty_per_level", "quantity per level"), ("total_budget", "total budget")] {
        if f(g, key) > 0.0 {
            b.num(format!("grid.{key}"), "grid", label.into(), f(g, key), "", 0.0);
        }
    }
}

/// Sizing, costs and portfolio limits that this strategy actually sets. A savings plan sizes
/// itself from its weights and its tranches, so the whole sizing block is dead config there and
/// offering it produced grids of identical trials.
fn money_params(b: &mut Builder, settings: &Value, is_dca: bool) {
    if let Some(sz) = settings.get("sizing").filter(|_| !is_dca) {
        match s(sz, "mode") {
            "percent_equity" => b.num("sizing.percent".into(), "sizing", "size (% of equity)".into(), f(sz, "percent"), "percent", 0.0),
            "fixed_qty" => b.num("sizing.qty".into(), "sizing", "quantity per entry".into(), f(sz, "qty"), "", 0.0),
            "risk" => b.num("sizing.risk_pct".into(), "sizing", "risk per trade".into(), f(sz, "risk_pct"), "percent", 0.0),
            "kelly" => {
                let fr = f(sz, "fraction");
                b.num("sizing.fraction".into(), "sizing", "Kelly fraction".into(), if fr > 0.0 { fr } else { 0.5 }, "fraction", 0.0);
                let w = f(sz, "window");
                b.int("sizing.window".into(), "sizing", "Kelly window".into(), if w > 0.0 { w } else { 30.0 }, 2.0);
                let cap = f(sz, "cap_pct");
                b.num("sizing.cap_pct".into(), "sizing", "Kelly cap".into(), if cap > 0.0 { cap } else { 20.0 }, "percent", 0.0);
            }
            // Equity tiers vary a table, not a number: an axis per row would be a different
            // strategy each trial, so the picker leaves it alone.
            _ => {}
        }
    }
    if !is_dca {
        let pyr = f(settings, "pyramiding");
        if pyr >= 1.0 {
            b.int("pyramiding".into(), "sizing", "max stacked entries".into(), pyr, 1.0);
        }
        let lev = f(settings, "leverage");
        if lev > 0.0 {
            b.num("leverage".into(), "sizing", "leverage".into(), lev, "", 1.0);
        }
    }
    // The money a savings plan starts with is one of its real parameters.
    if is_dca && f(settings, "starting_capital") > 0.0 {
        b.num(
            "starting_capital".into(),
            "dca",
            "starting capital".into(),
            f(settings, "starting_capital"),
            "",
            0.0,
        );
    }

    if let Some(fees) = settings.get("fees") {
        if f(fees, "amount") > 0.0 {
            let unit = if s(fees, "amount_kind") == "pct" { "percent" } else { "" };
            b.num("fees.amount".into(), "costs", "fee amount".into(), f(fees, "amount"), unit, 0.0);
        }
    }
    if f(settings, "spread_pct") > 0.0 {
        b.num("spread_pct".into(), "costs", "spread".into(), f(settings, "spread_pct"), "fraction", 0.0);
    }
    if let Some(sl) = settings.get("slippage") {
        if f(sl, "value") > 0.0 {
            let unit = if s(sl, "kind") == "ticks" { "ticks" } else { "fraction" };
            b.num("slippage.value".into(), "costs", "slippage".into(), f(sl, "value"), unit, 0.0);
        }
    }

    if let Some(risk) = settings.get("risk").filter(|_| !is_dca) {
        for (key, label) in [
            ("max_exposure_pct", "max total exposure"),
            ("max_exposure_per_asset_pct", "max exposure per asset"),
            ("max_daily_loss_pct", "max daily loss"),
            ("max_drawdown_pct", "max drawdown"),
        ] {
            if f(risk, key) > 0.0 {
                b.num(format!("risk.{key}"), "limits", label.into(), f(risk, key), "percent", 0.0);
            }
        }
        if f(risk, "max_open_positions") > 0.0 {
            b.int("risk.max_open_positions".into(), "limits", "max open positions".into(), f(risk, "max_open_positions"), 1.0);
        }
    }
    // Pyramiding is a signals-engine notion: a DCA tranche is not an add.
    if let Some(ps) = settings.get("pyramid_steps").filter(|_| !is_dca) {
        if f(ps, "min_distance_pct") > 0.0 {
            b.num("pyramid_steps.min_distance_pct".into(), "limits", "min distance between adds".into(), f(ps, "min_distance_pct"), "fraction", 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Writes what the engine discovers for the shared fixtures, for the browser half of the
    /// optimizer to diff itself against. The two implementations answer one question ("which
    /// parameters does this strategy expose?") and they have drifted before: the DCA axes landed
    /// here and never reached the picker, so the UI swept a savings plan on sizing knobs the mode
    /// ignores and every trial of the grid ran the same simulation.
    ///
    /// `scripts/optimizer-model/run.sh` runs this, then `check.mjs` compares.
    #[test]
    #[ignore]
    fn dump_paths_for_parity() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/optimizer-model");
        let src = std::fs::read_to_string(format!("{dir}/cases.json")).expect("cases.json");
        let cases: Value = serde_json::from_str(&src).unwrap();
        let mut out = serde_json::Map::new();
        for c in cases["cases"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap().to_string();
            let mut all: Vec<String> =
                discover(&c["settings"]).into_iter().flat_map(|p| p.paths).collect();
            all.sort();
            out.insert(name, json!(all));
        }
        std::fs::write(
            format!("{dir}/params_rust.json"),
            serde_json::to_string_pretty(&Value::Object(out)).unwrap() + "\n",
        )
        .unwrap();
    }

    fn paths(ps: &[Param]) -> Vec<String> {
        ps.iter().map(|p| p.paths.join("|")).collect()
    }

    #[test]
    fn reads_indicator_params_and_skips_unused_ones() {
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "entry": { "conditions": [
                { "left": { "kind": "indicator", "indicator": "rsi", "period": 14, "fast": 0, "mult": 0.0 },
                  "op": "below",
                  "right": { "kind": "const", "value": 30.0 } }
            ] } },
            "sizing": { "mode": "percent_equity", "percent": 10.0 }
        });
        let ps = discover(&s);
        let got = paths(&ps);
        assert!(got.contains(&"long.entry.conditions.0.left.period".to_string()));
        assert!(got.contains(&"long.entry.conditions.0.right.value".to_string()));
        assert!(!got.iter().any(|p| p.ends_with(".fast") || p.ends_with(".mult")));
        assert!(got.contains(&"sizing.percent".to_string()));
    }

    #[test]
    fn a_percent_stop_writes_both_spellings_and_stays_a_fraction() {
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "stop_loss": { "kind": "pct", "value": 0.02 } }
        });
        let p = &discover(&s)[0];
        assert_eq!(p.paths, vec!["long.stop_loss.value", "long.stop_loss_pct"]);
        assert_eq!(p.unit, "fraction");
        assert_eq!(p.value, 0.02);
        assert!(p.suggested.from > 0.0 && p.suggested.to > p.suggested.from);
    }

    #[test]
    fn the_inactive_side_is_not_offered() {
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "stop_loss_pct": 0.01 },
            "short": { "stop_loss_pct": 0.05 }
        });
        assert_eq!(paths(&discover(&s)), vec!["long.stop_loss_pct"]);
    }

    #[test]
    fn a_grid_offers_its_ladder_and_no_signals() {
        let s = json!({
            "kind": "grid", "mode": "long",
            "long": { "stop_loss_pct": 0.01 },
            "grid": { "levels": 10, "anchor": "ema", "anchor_period": 20, "width_kind": "pct", "width_value": 2.0, "qty_per_level": 0.5 }
        });
        let got = paths(&discover(&s));
        assert!(got.contains(&"grid.levels".to_string()));
        assert!(got.contains(&"grid.anchor_period".to_string()));
        assert!(got.contains(&"grid.qty_per_level".to_string()));
        assert!(!got.iter().any(|p| p.starts_with("long.")));
    }
}
