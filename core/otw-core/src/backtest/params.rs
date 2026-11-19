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
//!   it (the sizing mode in force, a stop that exists, an indicator param its indicator reads).
//!   Sweeping a field the engine ignores is the same dead grid as a misspelled path.
//!
//! Which params an indicator reads, and the default a 0 stands for, come from
//! `specs::INDICATOR_SPECS`. One length shared by several operands of an indicator family (the
//! four MACD operands of a cross) is one param writing every path.

use std::collections::BTreeMap;

use otw_script::input::InputValue;
use otw_script::output::{InputKind, ScriptMeta};
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
/// `scripts` holds the compiled metadata of the embedded scripts, by id.
pub fn discover(settings: &Value, scripts: &BTreeMap<String, ScriptMeta>) -> Vec<Param> {
    let mut b = Builder(Vec::new());
    let mode = s(settings, "mode");
    let kind = s(settings, "kind");
    let is_grid = kind == "grid";
    let is_dca = kind == "dca";

    if !is_grid && !is_dca {
        let active = |side: &str| {
            let off = (side == "long" && mode == "short") || (side == "short" && mode == "long");
            settings.get(side).filter(|_| !off)
        };
        let mirrored = match (active("long"), active("short")) {
            (Some(l), Some(sh)) => mirrors(l, sh),
            _ => false,
        };
        for side in ["long", "short"] {
            let Some(cfg) = active(side) else { continue };
            // The short side of a mirror is written by the long side's params.
            if !(mirrored && side == "short") {
                signal_params(&mut b, cfg, side, mirrored);
            }
            stop_params(&mut b, cfg, side);
        }
        if mirrored {
            link_mirrored_risk(&mut b);
        }
    }
    script_params(&mut b, settings, scripts);
    grid_params(&mut b, settings, is_grid);
    dca_params(&mut b, settings, is_dca);
    money_params(&mut b, settings, is_dca);
    b.0
}

/// Ids of the custom indicators an operand anywhere in `v` reads.
fn operand_ids<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::Object(m) => {
            if m.get("kind").and_then(Value::as_str) == Some("custom_indicator") {
                if let Some(id) = m.get("id").and_then(Value::as_str) {
                    out.push(id);
                }
            }
            for (k, x) in m {
                if k != "scripts" && k != "indicators" {
                    operand_ids(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| operand_ids(x, out)),
        _ => {}
    }
}

/// The number inputs of every script an operand reads. The suggested sweep stays inside the
/// input's `minval`/`maxval` and walks its `step` when it declares one: a value outside them
/// is refused by the engine, so a grid reaching past them would fail those trials.
fn script_params(b: &mut Builder, settings: &Value, scripts: &BTreeMap<String, ScriptMeta>) {
    let mut ids = Vec::new();
    operand_ids(settings, &mut ids);
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let (Some(def), Some(meta)) = (settings.get("scripts").and_then(|x| x.get(id)), scripts.get(id)) else { continue };
        let name = s(def, "name");
        for input in &meta.inputs {
            let kind = match input.kind {
                InputKind::Int => "int",
                InputKind::Float => "num",
                _ => continue,
            };
            let default = match input.default {
                InputValue::Int(x) => x as f64,
                InputValue::Float(x) => x,
                _ => 0.0,
            };
            let value = def.get("inputs").and_then(|i| i.get(&input.key)).and_then(Value::as_f64).unwrap_or(default);
            let (lo, hi) = (input.minval.unwrap_or(f64::NEG_INFINITY), input.maxval.unwrap_or(f64::INFINITY));
            let mut r = if kind == "int" { int_range(value, lo) } else { num_range(value, lo) };
            r.to = r.to.min(hi);
            r.from = r.from.max(lo).min(r.to);
            if let Some(step) = input.step.filter(|x| *x > 0.0) {
                r.step = step;
            }
            let path = format!("scripts.{id}.inputs.{}", input.key);
            if b.0.iter().any(|p| p.paths == [path.clone()]) {
                continue;
            }
            b.0.push(Param {
                paths: vec![path],
                group: "signals",
                label: format!("{name} {}", input.title),
                value: if kind == "int" { value.round() } else { tidy(value) },
                kind,
                unit: "",
                suggested: r,
            });
        }
    }
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

/// What each indicator param is called and how it varies: (key, label, kind).
const INDICATOR_KEYS: [(&str, &str, &str); 5] = [
    ("period", "period", "int"),
    ("fast", "fast length", "int"),
    ("slow", "slow length", "int"),
    ("signal_period", "signal length", "int"),
    ("mult", "multiplier", "num"),
];

/// The condition groups of a side the engine reads, with the path each one sits at: `entry`
/// (or the legacy single `signal` when `entry` is empty, as `Side::entry_group` folds it), then
/// `exit`. Each condition comes with its index and its label word ("entry", "exit").
fn side_conditions(side_cfg: &Value) -> Vec<(String, &'static str, usize, &Value)> {
    let conds = |g: &str| side_cfg.get(g).and_then(|g| g.get("conditions")).and_then(Value::as_array);
    let mut out = Vec::new();
    match conds("entry").filter(|c| !c.is_empty()) {
        Some(c) => out.extend(c.iter().enumerate().map(|(i, x)| (format!("entry.conditions.{i}"), "entry", i, x))),
        None => {
            if let Some(sig) = side_cfg.get("signal").filter(|v| v.is_object()) {
                out.push(("signal".to_string(), "entry", 0, sig));
            }
        }
    }
    if let Some(c) = conds("exit") {
        out.extend(c.iter().enumerate().map(|(i, x)| (format!("exit.conditions.{i}"), "exit", i, x)));
    }
    out
}

/// The comparison a short side makes where the long side makes `op`.
fn inverse_op(op: &str) -> &str {
    match op {
        "above" => "below",
        "below" => "above",
        "crosses_above" => "crosses_below",
        "crosses_below" => "crosses_above",
        "rising" => "falling",
        "falling" => "rising",
        "closing_above" => "closing_below",
        "closing_below" => "closing_above",
        "opening_above" => "opening_below",
        "opening_below" => "opening_above",
        other => other,
    }
}

/// Whether the short side is the long side written out as its mirror: the same conditions at
/// the same places, each with the inverse comparison (or the same one, for a filter with no
/// direction such as "ADX above 25"), the same series, and constants free to differ (a
/// threshold mirrors about the oscillator's centre, RSI 30 becoming 70). The two sides then
/// hold one strategy, and each of its indicator lengths has to move on both at once.
fn mirrors(long: &Value, short: &Value) -> bool {
    let (lg, sg) = (side_conditions(long), side_conditions(short));
    if lg.is_empty() || lg.len() != sg.len() {
        return false;
    }
    let same_operand = |a: Option<&Value>, b: Option<&Value>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) if a.get("kind") == b.get("kind") => s(a, "kind") == "const" || a == b,
        _ => false,
    };
    lg.iter().zip(&sg).all(|((lp, _, _, lc), (sp, _, _, sc))| {
        let (lo, so) = (s(lc, "op"), s(sc, "op"));
        lp == sp
            && (so == lo || so == inverse_op(lo))
            && same_operand(lc.get("left"), sc.get("left"))
            && same_operand(lc.get("right"), sc.get("right"))
    })
}

/// One indicator length shared by every operand that holds it.
struct Linked {
    family: &'static str,
    key: &'static str,
    value: f64,
    /// Indicator names seen, to name the parameter by its indicator when only one is involved.
    names: Vec<String>,
    /// Paths on the side being walked; the mirror's paths are added at emission.
    paths: Vec<String>,
    /// Where the first operand sits ("entry 1"), for the label of a lone operand.
    first_at: String,
}

/// Indicator and threshold parameters of one side's signals.
///
/// An indicator param is linked across every operand of the side that holds the same value
/// for it within one family: `macd` and `macd_signal` are views of one MACD, so a cross of the
/// two holds the fast length in four operands (entry and exit, left and right), and sweeping
/// one of them alone measures a strategy nobody configured. Only the params the indicator
/// reads are offered (its row in `specs::INDICATOR_SPECS`), and one left at 0 is offered at
/// the default the 0 stands for. With `mirror` (the short side is this side's mirror, see
/// [`mirrors`]) every path is written on both sides.
fn signal_params(b: &mut Builder, side_cfg: &Value, side: &str, mirror: bool) {
    use super::specs;
    let conds = side_conditions(side_cfg);
    let mut linked: Vec<Linked> = Vec::new();
    for (base, word, i, cond) in &conds {
        for slot in ["left", "right"] {
            let Some(o) = cond.get(slot).filter(|o| s(o, "kind") == "indicator") else { continue };
            let name = s(o, "indicator");
            let Some(spec) = specs::spec(name) else { continue };
            for (key, default) in spec.params {
                let Some((key, _, _)) = INDICATOR_KEYS.iter().find(|(k, _, _)| k == key) else { continue };
                let given = f(o, key);
                let value = tidy(if given > 0.0 { given } else { *default });
                let path = format!("{side}.{base}.{slot}.{key}");
                match linked.iter_mut().find(|l| l.family == spec.family && l.key == *key && l.value == value) {
                    Some(l) => {
                        if !l.names.iter().any(|n| n == name) {
                            l.names.push(name.to_string());
                        }
                        l.paths.push(path);
                    }
                    None => linked.push(Linked {
                        family: spec.family,
                        key,
                        value,
                        names: vec![name.to_string()],
                        paths: vec![path],
                        first_at: format!("{word} {}", i + 1),
                    }),
                }
            }
        }
    }

    let who = if mirror { "long and short".to_string() } else { side.to_string() };
    let twin = |paths: &[String]| -> Vec<String> {
        let mut out = paths.to_vec();
        if mirror {
            out.extend(paths.iter().filter_map(|p| p.strip_prefix("long.")).map(|t| format!("short.{t}")));
        }
        out
    };
    let mut emitted = vec![false; linked.len()];
    for (base, word, i, cond) in &conds {
        for slot in ["left", "right"] {
            let Some(o) = cond.get(slot) else { continue };
            let path = format!("{side}.{base}.{slot}");
            match s(o, "kind") {
                "indicator" => {
                    // Emit each linked param where its first operand sits, so the order reads
                    // like the strategy.
                    for (k, l) in linked.iter().enumerate() {
                        if emitted[k] || !l.paths.iter().any(|p| p.starts_with(&format!("{path}."))) {
                            continue;
                        }
                        emitted[k] = true;
                        let (_, label, kind) = INDICATOR_KEYS.iter().find(|(key, _, _)| *key == l.key).copied().unwrap_or(("", "", "int"));
                        let name = if l.names.len() == 1 { l.names[0].to_uppercase() } else { l.family.to_uppercase() };
                        let n = l.paths.len();
                        let text = if n == 1 {
                            format!("{name} {label} ({who} {})", l.first_at)
                        } else if mirror {
                            format!("{name} {label} ({n} operands, long and short)")
                        } else {
                            format!("{name} {label} ({n} operands)")
                        };
                        let (unit, min) = if kind == "int" { ("", 1.0) } else { ("multiple", 0.0) };
                        b.push(&twin(&l.paths), "signals", text, l.value, kind, unit, min);
                    }
                }
                // A custom indicator carries a definition, not params: nothing to vary from here.
                // A threshold is never linked: a mirrored one is a different number on the
                // short side (RSI 30 against 70).
                "const" => b.num(
                    format!("{path}.value"),
                    "signals",
                    format!("threshold ({side} {word} {})", i + 1),
                    f(o, "value"),
                    "",
                    f64::NEG_INFINITY,
                ),
                _ => {}
            }
        }
    }
}

/// On a mirrored strategy, a short stop holding the same value as the long one moves with it:
/// the long param gains the short paths and the short param goes.
fn link_mirrored_risk(b: &mut Builder) {
    let mut k = 0;
    while k < b.0.len() {
        let p = &b.0[k];
        let tails: Option<Vec<&str>> = p.paths.iter().map(|x| x.strip_prefix("short.")).collect();
        let found = (p.group == "risk").then_some(()).and(tails).and_then(|tails| {
            b.0.iter().position(|q| {
                q.group == "risk"
                    && q.value == p.value
                    && q.paths.len() == tails.len()
                    && q.paths.iter().zip(&tails).all(|(qp, t)| qp.strip_prefix("long.") == Some(*t))
            })
        });
        match found {
            Some(at) => {
                let short = b.0.remove(k);
                let at = if at > k { at - 1 } else { at };
                let long = &mut b.0[at];
                long.paths.extend(short.paths);
                if let Some(rest) = long.label.strip_prefix("long ") {
                    long.label = format!("long and short {rest}");
                }
            }
            None => k += 1,
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
                discover(&c["settings"], &BTreeMap::new()).into_iter().flat_map(|p| p.paths).collect();
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
        let ps = discover(&s, &BTreeMap::new());
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
        let p = &discover(&s, &BTreeMap::new())[0];
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
        assert_eq!(paths(&discover(&s, &BTreeMap::new())), vec!["long.stop_loss_pct"]);
    }

    #[test]
    fn a_grid_offers_its_ladder_and_no_signals() {
        let s = json!({
            "kind": "grid", "mode": "long",
            "long": { "stop_loss_pct": 0.01 },
            "grid": { "levels": 10, "anchor": "ema", "anchor_period": 20, "width_kind": "pct", "width_value": 2.0, "qty_per_level": 0.5 }
        });
        let got = paths(&discover(&s, &BTreeMap::new()));
        assert!(got.contains(&"grid.levels".to_string()));
        assert!(got.contains(&"grid.anchor_period".to_string()));
        assert!(got.contains(&"grid.qty_per_level".to_string()));
        assert!(!got.iter().any(|p| p.starts_with("long.")));
    }

    const INPUTS: &str = "//@version=6\nindicator(\"Band\")\nlen = input.int(14, \"Length\", minval = 2, maxval = 20, step = 2)\nmult = input.float(2.0, \"Mult\")\nsrc = input.source(close, \"Source\")\nplot(ta.sma(src, len) * mult, \"mid\")";

    fn script_settings(operand: bool) -> (Value, BTreeMap<String, ScriptMeta>) {
        let op = json!({ "kind": "custom_indicator", "id": "s1", "plot": "mid" });
        let left = if operand { op } else { json!({ "kind": "price", "field": "close" }) };
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "entry": { "logic": "all", "conditions": [{ "left": left, "op": "above", "right": { "kind": "const", "value": 1.0 } }] } },
            "scripts": { "s1": { "name": "Band", "source": INPUTS, "inputs": { "mult": 2.5 } } }
        });
        let meta = otw_script::compile(INPUTS).unwrap().meta().clone();
        (s, BTreeMap::from([("s1".to_string(), meta)]))
    }

    /// Phase 3: the number inputs of a script an operand reads are grid axes; a source input is not.
    #[test]
    fn script_inputs_are_params() {
        let (s, metas) = script_settings(true);
        let ps: Vec<Param> = discover(&s, &metas).into_iter().filter(|p| p.paths[0].starts_with("scripts.")).collect();
        assert_eq!(ps.len(), 2, "{ps:?}");
        let len = &ps[0];
        assert_eq!(len.paths, vec!["scripts.s1.inputs.len"]);
        assert_eq!((len.group, len.kind, len.unit, len.label.as_str(), len.value), ("signals", "int", "", "Band Length", 14.0));
        assert_eq!((len.suggested.from, len.suggested.to, len.suggested.step), (7.0, 20.0, 2.0));
        let mult = &ps[1];
        assert_eq!((mult.paths[0].as_str(), mult.kind, mult.value), ("scripts.s1.inputs.mult", "num", 2.5));
    }

    #[test]
    fn script_input_unused_script() {
        let (s, metas) = script_settings(false);
        assert!(!discover(&s, &metas).iter().any(|p| p.paths[0].starts_with("scripts.")));
    }

    fn macd(name: &str) -> Value {
        if name == "macd" {
            json!({ "kind": "indicator", "indicator": "macd", "fast": 12, "slow": 26 })
        } else {
            json!({ "kind": "indicator", "indicator": name, "fast": 12, "slow": 26, "signal_period": 9 })
        }
    }

    /// The audit's reference strategy: MACD 12/26/9, line crosses above the signal to enter,
    /// below to exit, 7% stop.
    fn macd_cross(mode: &str) -> Value {
        let side = |up: &str, down: &str| {
            json!({
                "entry": { "logic": "all", "conditions": [{ "left": macd("macd"), "op": up, "right": macd("macd_signal") }] },
                "exit": { "logic": "all", "conditions": [{ "left": macd("macd"), "op": down, "right": macd("macd_signal") }] },
                "stop_loss": { "kind": "pct", "value": 0.07 },
                "stop_loss_pct": 0.07
            })
        };
        json!({
            "kind": "signals", "mode": mode,
            "long": side("crosses_above", "crosses_below"),
            "short": side("crosses_below", "crosses_above"),
            "sizing": { "mode": "percent_equity", "percent": 100.0 }
        })
    }

    /// Phase 8: one MACD length is one axis writing every operand that holds it. The macd line
    /// does not read a signal length, so that axis has only the two macd_signal operands.
    #[test]
    fn a_macd_cross_links_its_lengths_across_operands() {
        let ps = discover(&macd_cross("long"), &BTreeMap::new());
        let signals: Vec<&Param> = ps.iter().filter(|p| p.group == "signals").collect();
        assert_eq!(signals.len(), 3, "{signals:#?}");
        let fast = signals.iter().find(|p| p.paths[0].ends_with(".fast")).unwrap();
        assert_eq!(
            fast.paths,
            [
                "long.entry.conditions.0.left.fast",
                "long.entry.conditions.0.right.fast",
                "long.exit.conditions.0.left.fast",
                "long.exit.conditions.0.right.fast",
            ]
        );
        assert_eq!((fast.label.as_str(), fast.value), ("MACD fast length (4 operands)", 12.0));
        let slow = signals.iter().find(|p| p.paths[0].ends_with(".slow")).unwrap();
        assert_eq!((slow.paths.len(), slow.value), (4, 26.0));
        let sig = signals.iter().find(|p| p.paths[0].ends_with(".signal_period")).unwrap();
        assert_eq!(sig.paths, ["long.entry.conditions.0.right.signal_period", "long.exit.conditions.0.right.signal_period"]);
        assert_eq!(sig.label, "MACD_SIGNAL signal length (2 operands)");
        let stop: Vec<&Param> = ps.iter().filter(|p| p.group == "risk").collect();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0].paths, ["long.stop_loss.value", "long.stop_loss_pct"]);
    }

    /// Long and short are linked when the short side is the long one mirrored.
    #[test]
    fn a_mirrored_short_side_moves_with_the_long_one() {
        let ps = discover(&macd_cross("both"), &BTreeMap::new());
        let signals: Vec<&Param> = ps.iter().filter(|p| p.group == "signals").collect();
        assert_eq!(signals.len(), 3, "{signals:#?}");
        let fast = signals.iter().find(|p| p.paths[0].ends_with(".fast")).unwrap();
        assert_eq!(fast.paths.len(), 8);
        assert!(fast.paths.contains(&"short.exit.conditions.0.right.fast".to_string()));
        assert_eq!(fast.label, "MACD fast length (4 operands, long and short)");
        let risk: Vec<&Param> = ps.iter().filter(|p| p.group == "risk").collect();
        assert_eq!(risk.len(), 1, "{risk:#?}");
        assert_eq!(risk[0].paths, ["long.stop_loss.value", "long.stop_loss_pct", "short.stop_loss.value", "short.stop_loss_pct"]);
        assert_eq!(risk[0].label, "long and short stop loss");

        // A short side that is not the mirror keeps its own axes.
        let mut s = macd_cross("both");
        s["short"]["entry"]["conditions"][0]["right"] = json!({ "kind": "indicator", "indicator": "ema", "period": 50 });
        let ps = discover(&s, &BTreeMap::new());
        assert!(ps.iter().any(|p| p.paths.iter().all(|x| x.starts_with("short."))), "{ps:#?}");
        assert!(ps.iter().all(|p| !(p.paths.iter().any(|x| x.starts_with("long.")) && p.paths.iter().any(|x| x.starts_with("short.")))));
    }

    /// Two lengths of one family holding different values are two axes, each on its operands.
    #[test]
    fn an_sma_cross_has_one_axis_per_length() {
        let sma = |p: u32| json!({ "kind": "indicator", "indicator": "sma", "period": p });
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": {
                "entry": { "conditions": [{ "left": sma(12), "op": "crosses_above", "right": sma(50) }] },
                "exit": { "conditions": [{ "left": sma(12), "op": "crosses_below", "right": sma(50) }] }
            }
        });
        let ps: Vec<Param> = discover(&s, &BTreeMap::new()).into_iter().filter(|p| p.group == "signals").collect();
        assert_eq!(ps.len(), 2, "{ps:#?}");
        assert_eq!(ps[0].paths, ["long.entry.conditions.0.left.period", "long.exit.conditions.0.left.period"]);
        assert_eq!((ps[0].value, ps[0].label.as_str()), (12.0, "SMA period (2 operands)"));
        assert_eq!(ps[1].paths, ["long.entry.conditions.0.right.period", "long.exit.conditions.0.right.period"]);
        assert_eq!(ps[1].value, 50.0);
    }

    /// A param left at 0 runs at its default and is offered there; a param the indicator does
    /// not read is not offered, whatever it holds.
    #[test]
    fn a_zero_param_is_offered_at_its_default_and_an_unread_one_never() {
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "entry": { "conditions": [{
                "left": { "kind": "indicator", "indicator": "bb_upper", "period": 0, "mult": 0.0, "fast": 9 },
                "op": "below",
                "right": { "kind": "price", "field": "close" }
            }] } }
        });
        let ps = discover(&s, &BTreeMap::new());
        let got: Vec<(String, f64)> = ps.iter().map(|p| (p.paths.join("|"), p.value)).collect();
        assert_eq!(
            got,
            [("long.entry.conditions.0.left.period".to_string(), 20.0), ("long.entry.conditions.0.left.mult".to_string(), 2.0)]
        );
        assert_eq!(ps[0].label, "BB_UPPER period (long entry 1)");
    }

    /// The legacy single `signal` is read when `entry` is empty, so its lengths are params.
    #[test]
    fn a_legacy_signal_is_read_when_entry_is_empty() {
        let s = json!({
            "kind": "signals", "mode": "long",
            "long": { "signal": { "left": { "kind": "indicator", "indicator": "rsi", "period": 14 }, "op": "below",
                                  "right": { "kind": "const", "value": 30.0 } } }
        });
        assert_eq!(
            paths(&discover(&s, &BTreeMap::new())),
            ["long.signal.left.period", "long.signal.right.value"]
        );
    }
}
