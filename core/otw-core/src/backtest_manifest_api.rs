//! `GET /api/backtest/manifest`: everything a caller needs to compose engine settings, in one
//! read, generated from the code that enforces it.
//!
//! - operands and their parameters, from `specs::INDICATOR_SPECS`, `METRICS` and the schema;
//! - the closed vocabularies, read from the `Settings` schema (`enum` and tagged `oneOf`) plus
//!   the string fields `Settings::validate` matches by hand (a test pins those);
//! - units, discovered by running `params::discover` over probe settings, so a new `_pct`
//!   parameter the optimizer learns about appears here (and in the catalog prose) on its own;
//! - the engine limits, and one runnable example (`specs::EXAMPLE_SETTINGS`).
//!
//! `otw_catalog {"module":"backtest"}` appends [`compact`], the same content in a few lines.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use axum::{routing::get, Json, Router};
use serde_json::{json, Map, Value};

use crate::backtest::{operand_check, optimize, params, specs};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/backtest/manifest", get(manifest_handler))
}

async fn manifest_handler() -> Json<Value> {
    Json(manifest().clone())
}

/// Datasets one run may read (`RunBody::ids` in `backtest_api.rs`).
pub const MAX_DATASETS: usize = 8;
/// Axes and trials of one `/api/backtest/sweep` (`MAX_SWEEP_AXES` / `MAX_SWEEP_TRIALS`).
pub const SWEEP_AXES: usize = 4;
pub const SWEEP_TRIALS: usize = 64;

/// String fields `Settings::validate` matches by hand: the schema says "string", the engine
/// refuses anything else. A test runs each value through `validate`.
const STRING_VOCABS: &[(&str, &[&str])] = &[
    ("kind", &["signals", "grid", "dca"]),
    ("fees.amount_kind", &["pct", "fixed"]),
    ("fees.per", &["trade", "unit"]),
    ("slippage.kind", &["pct", "ticks"]),
    ("long|short.stop_loss.kind, take_profit.kind", &["pct", "atr"]),
    ("long|short.trailing_stop.kind", &["pct", "abs"]),
];

/// What each metric operand reads: whether it needs `period`, and the unit of its value.
const METRIC_INFO: &[(&str, bool, &str)] = &[
    ("dd_from_high", false, "percent"),
    ("up_from_low", false, "percent"),
    ("change_pct", true, "percent"),
    ("rel_volume", true, "ratio"),
    ("change_from_start", false, "percent"),
];

/// Fields readable by a `position` operand, inside a DCA run only (`dca::POS_FIELDS`).
const POSITION_FIELDS: &[&str] =
    &["pnl_pct", "since_last_buy_pct", "avg_cost", "units", "value", "weight_pct", "cash_pct", "drawdown_pct"];

/// Units of the settings fields the optimizer does not vary, so `params::discover` never
/// reports them. (path, unit, note)
const EXTRA_UNITS: &[(&str, &str, &str)] = &[
    ("oos_split_pct", "fraction", "the fraction of bars kept in-sample; the tail is out-of-sample"),
    ("funding.annual_rate_pct", "percent", ""),
    ("execution.maker_fee", "percent", "same unit as fees.amount"),
];

/// One unit row: the unit under the probe's default kinds, and the other units the same path
/// takes under another kind, each with the condition that selects it.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitRow {
    pub unit: &'static str,
    pub note: &'static str,
    pub alternates: Vec<(&'static str, &'static str)>,
}

/// Settings that set every parameter `params::discover` can report, once per kind. The first
/// probe of each strategy kind (empty condition) is its base; the others change one kind and
/// name it, so a path whose unit depends on a kind says which.
fn probes() -> Vec<(&'static str, Value)> {
    let side = |stop: Value, trail: Value| {
        json!({
            "entry": {"logic": "all", "conditions": [
                {"left": {"kind": "indicator", "indicator": "rsi", "period": 14}, "op": "below",
                 "right": {"kind": "const", "value": 30}}]},
            "stop_loss": stop.clone(), "take_profit": stop, "trailing_stop": trail
        })
    };
    let pct_stop = json!({"kind": "pct", "value": 0.02});
    let pct_trail = json!({"kind": "pct", "value": 0.02, "activate_pct": 0.01, "breakeven_pct": 0.005});
    let signals = |patch: Value| {
        let mut s = json!({
            "kind": "signals", "mode": "both",
            "long": side(pct_stop.clone(), pct_trail.clone()),
            "short": side(pct_stop.clone(), pct_trail.clone()),
            "pyramiding": 2, "leverage": 2,
            "sizing": {"mode": "percent_equity", "percent": 10},
            "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
            "spread_pct": 0.001,
            "slippage": {"kind": "pct", "value": 0.0005},
            "risk": {"max_exposure_pct": 50, "max_exposure_per_asset_pct": 20, "max_daily_loss_pct": 5,
                     "max_drawdown_pct": 20, "max_open_positions": 3},
            "pyramid_steps": {"min_distance_pct": 0.01}
        });
        if let (Some(o), Value::Object(p)) = (s.as_object_mut(), patch) {
            o.extend(p);
        }
        s
    };
    let atr = json!({"kind": "atr", "value": 2, "period": 14});
    let abs_trail = json!({"kind": "abs", "value": 5});
    let grid = |width_kind: &str| {
        json!({"kind": "grid", "mode": "long", "grid": {"levels": 10, "anchor": "ema", "anchor_period": 20,
               "width_kind": width_kind, "width_value": 2, "width_period": 14, "qty_per_level": 1}})
    };
    let dca = |buy_kind: &str, sell_kind: &str| {
        json!({"kind": "dca", "mode": "long", "dca": {
            "contribution": {"amount": 100, "period": "month", "every": 1},
            "buys": [{"amount": 10, "amount_kind": buy_kind}],
            "sells": [{"amount": 50, "amount_kind": sell_kind, "target_gain_pct": 20}]}})
    };
    vec![
        ("", signals(json!({}))),
        ("sizing.mode=risk", signals(json!({"sizing": {"mode": "risk", "risk_pct": 1}}))),
        ("sizing.mode=kelly", signals(json!({"sizing": {"mode": "kelly", "fraction": 0.5, "window": 30, "cap_pct": 20}}))),
        ("fees.amount_kind=fixed", signals(json!({"fees": {"amount_kind": "fixed", "per": "trade", "amount": 1}}))),
        ("slippage.kind=ticks", signals(json!({"slippage": {"kind": "ticks", "value": 2}}))),
        ("kind=atr", signals(json!({"long": side(atr.clone(), pct_trail.clone()), "short": side(atr, pct_trail.clone())}))),
        ("kind=abs", signals(json!({"long": side(pct_stop.clone(), abs_trail.clone()), "short": side(pct_stop.clone(), abs_trail)}))),
        ("", grid("pct")),
        ("grid.width_kind=atr", grid("atr")),
        ("", dca("pct_cash", "pct_position")),
        ("amount_kind=fixed", dca("fixed", "pct_position")),
        ("amount_kind=units", dca("pct_cash", "units")),
    ]
}

/// An array index becomes `*`: `dca.buys.0.amount` is `dca.buys.*.amount`.
fn normalize(path: &str) -> String {
    path.split('.')
        .map(|s| if s.bytes().all(|b| b.is_ascii_digit()) { "*" } else { s })
        .collect::<Vec<_>>()
        .join(".")
}

/// Every settings path whose value is a fraction or a percent, with its unit. Computed once.
pub fn unit_table() -> &'static BTreeMap<String, UnitRow> {
    static TABLE: OnceLock<BTreeMap<String, UnitRow>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut base: BTreeMap<String, &'static str> = BTreeMap::new();
        let mut alts: BTreeMap<String, Vec<(&'static str, &'static str)>> = BTreeMap::new();
        let mut seen: Vec<(&'static str, Vec<(String, &'static str)>)> = Vec::new();
        for (cond, s) in probes() {
            let found = params::discover(&s, &BTreeMap::new())
                .into_iter()
                .flat_map(|p| p.paths.into_iter().map(move |path| (normalize(&path), p.unit)))
                .collect::<Vec<_>>();
            seen.push((cond, found));
        }
        for (cond, found) in &seen {
            if cond.is_empty() {
                for (path, unit) in found {
                    base.entry(path.clone()).or_insert(unit);
                }
            }
        }
        for (cond, found) in &seen {
            for (path, unit) in found {
                match base.get(path) {
                    Some(b) if b == unit => {}
                    _ => {
                        let list = alts.entry(path.clone()).or_default();
                        if !list.iter().any(|(u, _)| u == unit) {
                            list.push((unit, cond));
                        }
                    }
                }
            }
        }
        let family = |u: &str| matches!(u, "fraction" | "percent");
        let mut out = BTreeMap::new();
        for (path, unit) in &base {
            let alternates = alts.remove(path).unwrap_or_default();
            if family(unit) || alternates.iter().any(|(u, _)| family(u)) {
                out.insert(path.clone(), UnitRow { unit, note: "", alternates });
            }
        }
        // A path no base probe reached (only a variant did) keeps the first unit it showed.
        for (path, mut alternates) in alts {
            let (unit, _) = alternates.remove(0);
            if family(unit) || alternates.iter().any(|(u, _)| family(u)) {
                out.insert(path, UnitRow { unit, note: "", alternates });
            }
        }
        for (path, unit, note) in EXTRA_UNITS {
            out.insert(path.to_string(), UnitRow { unit, note, alternates: Vec::new() });
        }
        out
    })
}

fn unit_word(u: &str) -> &str {
    if u.is_empty() {
        "plain"
    } else {
        u
    }
}

/// One row as prose: `slippage.value (ticks when slippage.kind=ticks)`.
fn describe(path: &str, row: &UnitRow) -> String {
    let mut extra: Vec<String> = Vec::new();
    if !row.note.is_empty() {
        extra.push(row.note.to_string());
    }
    for (u, cond) in &row.alternates {
        extra.push(format!("{} when {cond}", unit_word(u)));
    }
    if extra.is_empty() {
        path.to_string()
    } else {
        format!("{path} ({})", extra.join(", "))
    }
}

/// The FRACTIONS / PERCENTS lists, `long.x` and `short.x` written once as `long/short.x`.
pub fn units_paragraph() -> String {
    let table = unit_table();
    let mut fractions = Vec::new();
    let mut percents = Vec::new();
    for (path, row) in table {
        if path.starts_with("short.") && table.contains_key(&format!("long.{}", &path[6..])) {
            continue;
        }
        let shown = match path.strip_prefix("long.") {
            Some(rest) if table.contains_key(&format!("short.{rest}")) => format!("long/short.{rest}"),
            _ => path.clone(),
        };
        let text = describe(&shown.replace(".*.", "[*]."), row);
        match row.unit {
            "fraction" => fractions.push(text),
            "percent" => percents.push(text),
            _ => {}
        }
    }
    format!(
        "FRACTIONS (0.01 = 1%): {}.\nPERCENTS (10 = 10%): {}.",
        fractions.join(", "),
        percents.join(", ")
    )
}

/// Values of a closed schema type: `enum`, or the `const` of each `oneOf` branch.
fn enum_values(def: &Value) -> Vec<Value> {
    if let Some(Value::Array(e)) = def.get("enum") {
        return e.clone();
    }
    def.get("oneOf")
        .and_then(Value::as_array)
        .map(|bs| {
            bs.iter()
                .filter_map(|b| b.get("const").or_else(|| b.get("enum").and_then(|e| e.get(0))).cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// The tag of each `oneOf` branch of an internally tagged enum (`kind`, `mode`).
fn tags(def: &Value, key: &str) -> Vec<Value> {
    def.get("oneOf")
        .and_then(Value::as_array)
        .map(|bs| bs.iter().filter_map(|b| b.pointer(&format!("/properties/{key}/const")).cloned()).collect())
        .unwrap_or_default()
}

/// The manifest, built once.
pub fn manifest() -> &'static Value {
    static MANIFEST: OnceLock<Value> = OnceLock::new();
    MANIFEST.get_or_init(build)
}

fn build() -> Value {
    let schema = crate::mcp::catalog::schema::<crate::backtest::Settings>();
    let def = |name: &str| schema.pointer(&format!("/definitions/{name}")).cloned().unwrap_or(Value::Null);

    let indicators: Vec<Value> = specs::INDICATOR_SPECS
        .iter()
        .map(|s| {
            let params: Map<String, Value> = s.params.iter().map(|(k, d)| (k.to_string(), json!(d))).collect();
            json!({"name": s.name, "params": params, "chainable": s.chainable, "output": s.output, "needs_ohlc": s.needs_ohlc})
        })
        .collect();
    let metrics: Vec<Value> = METRIC_INFO
        .iter()
        .map(|(name, period, unit)| {
            let params = if *period { json!({"period": format!("1 to {}", operand_check::MAX_PERIOD)}) } else { json!({}) };
            json!({"name": name, "params": params, "unit": unit})
        })
        .collect();

    let mut enums = Map::new();
    enums.insert("mode".into(), json!(enum_values(&def("Mode"))));
    enums.insert("logic".into(), json!(enum_values(&def("Logic"))));
    enums.insert("price.field".into(), json!(enum_values(&def("PriceField"))));
    enums.insert("sizing.mode".into(), json!(tags(&def("Sizing"), "mode")));
    enums.insert("sizing.metric (equity_tiers)".into(), json!(enum_values(&def("TierMetric"))));
    enums.insert("pyramid_steps.after_add_sl".into(), json!(enum_values(&def("AfterAddSl"))));
    for (path, values) in STRING_VOCABS {
        enums.insert(path.to_string(), json!(values));
    }

    let units: Map<String, Value> = unit_table()
        .iter()
        .map(|(path, row)| {
            let mut text = unit_word(row.unit).to_string();
            if !row.note.is_empty() {
                text.push_str(&format!(", {}", row.note));
            }
            for (u, cond) in &row.alternates {
                text.push_str(&format!("; {} when {cond}", unit_word(u)));
            }
            (path.clone(), json!(text))
        })
        .collect();

    json!({
        "operands": {
            "price": {"field": enum_values(&def("PriceField"))},
            "const": {"value": "number"},
            "indicator": indicators,
            "metric": metrics,
            "position": {"field": POSITION_FIELDS, "note": "DCA runs only"},
            "custom_indicator": {"id": "embedded in settings.indicators (node graph) or settings.scripts (script, read through `plot`)"}
        },
        "ops": enum_values(&def("Op")),
        "enums": enums,
        "units": units,
        "limits": {
            "max_datasets": MAX_DATASETS,
            "sweep_axes": SWEEP_AXES,
            "sweep_trials": SWEEP_TRIALS,
            "max_period": operand_check::MAX_PERIOD,
            "optimize_axes": optimize::MAX_AXES,
            "optimize_axis_values": optimize::MAX_AXIS_VALUES,
            "optimize_trials": optimize::MAX_TRIALS,
            "optimize_points": optimize::MAX_POINTS
        },
        "example": specs::example_settings()
    })
}

fn joined(v: &Value) -> String {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join("|"))
        .unwrap_or_default()
}

/// The manifest in a few lines, for `otw_catalog {"module":"backtest"}`. The example and the
/// unit lists are left out: the listing it is appended to carries the units in the
/// `POST /api/backtest/run` description, and that endpoint's detail prints the example.
pub fn compact() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let m = manifest();
        let num = |d: f64| if d.fract() == 0.0 { format!("{d:.0}") } else { d.to_string() };
        // Grouped by parameter set, in table order: most built-ins share one of a few.
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for s in specs::INDICATOR_SPECS {
            let p = s.params.iter().map(|(k, d)| format!("{k}={}", num(*d))).collect::<Vec<_>>().join(",");
            let name = format!("{}{}", s.name, if s.chainable { "*" } else { "" });
            match groups.iter_mut().find(|(g, _)| *g == p) {
                Some((_, names)) => names.push(name),
                None => groups.push((p, vec![name])),
            }
        }
        let indicators = groups
            .iter()
            .map(|(p, names)| format!("({p}) {}", names.join(" ")))
            .collect::<Vec<_>>()
            .join("; ");
        let metrics = METRIC_INFO
            .iter()
            .map(|(n, p, u)| format!("{n}{} {u}", if *p { "(period)" } else { "" }))
            .collect::<Vec<_>>()
            .join(", ");
        let enums = m["enums"]
            .as_object()
            .map(|o| o.iter().map(|(k, v)| format!("{k} {}", joined(v))).collect::<Vec<_>>().join("; "))
            .unwrap_or_default();
        let l = &m["limits"];
        format!(
            "\n## backtest manifest (JSON: GET /api/backtest/manifest)\n\
             operands: price {{field}}, const {{value}}, indicator {{indicator, period, fast, slow, signal_period, mult}}, \
             custom_indicator {{id, plot}}, metric {{metric, period}}, position {{field}} (DCA only). \
             0 = the default shown; a param not listed is refused.\n\
             indicators by params (* = chainable): {indicators}\n\
             metrics: {metrics}\n\
             position fields: {}\n\
             ops: {}\n\
             enums: {enums}\n\
             units: the FRACTIONS / PERCENTS lists of POST /api/backtest/run.\n\
             limits: {} datasets/run, sweep {} axes {} trials, period <= {}, optimizer {} axes {} trials {} points\n",
            POSITION_FIELDS.join(", "),
            joined(&m["ops"]),
            l["max_datasets"], l["sweep_axes"], l["sweep_trials"], l["max_period"],
            l["optimize_axes"], l["optimize_trials"], l["optimize_points"],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::{Settings, METRICS};
    use std::collections::BTreeSet;

    #[test]
    fn backtest_manifest_lists_macd_with_its_defaults() {
        let m = manifest();
        let macd = m["operands"]["indicator"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "macd")
            .expect("macd row");
        assert_eq!(macd["params"]["fast"], 12.0);
        assert_eq!(macd["params"]["slow"], 26.0);
        assert_eq!(macd["chainable"], true);
        assert_eq!(m["operands"]["indicator"].as_array().unwrap().len(), specs::INDICATOR_SPECS.len());
        for key in ["operands", "ops", "enums", "units", "limits", "example"] {
            assert!(m.get(key).is_some(), "manifest misses {key}");
        }
        assert_eq!(m["enums"]["mode"], json!(["long", "short", "both"]));
        assert_eq!(m["enums"]["logic"], json!(["all", "any"]));
        assert!(joined(&m["ops"]).contains("crosses_above"));
        assert!(joined(&m["enums"]["sizing.mode"]).contains("percent_equity"));
        let kinds = tags(&crate::mcp::catalog::schema::<Settings>()["definitions"]["Operand"], "kind");
        let listed: Vec<&String> = m["operands"].as_object().unwrap().keys().collect();
        assert_eq!(kinds.len(), listed.len(), "every operand kind has an entry: {listed:?}");
        for k in &kinds {
            assert!(m["operands"].get(k.as_str().unwrap()).is_some(), "operand kind {k} missing");
        }
        assert_eq!(m["example"]["oos_split_pct"], 0.7);
    }

    #[test]
    fn backtest_manifest_units_come_from_the_optimizer() {
        let t = unit_table();
        assert_eq!(t["long.stop_loss_pct"].unit, "fraction");
        assert_eq!(t["spread_pct"].unit, "fraction");
        assert_eq!(t["sizing.percent"].unit, "percent");
        assert_eq!(t["sizing.cap_pct"].unit, "percent");
        assert_eq!(t["sizing.fraction"].unit, "fraction");
        assert_eq!(t["oos_split_pct"].unit, "fraction");
        assert_eq!(t["fees.amount"].unit, "percent");
        assert_eq!(t["fees.amount"].alternates, vec![("", "fees.amount_kind=fixed")]);
        assert_eq!(t["slippage.value"].alternates, vec![("ticks", "slippage.kind=ticks")]);
        assert_eq!(t["dca.sells.*.target_gain_pct"].unit, "percent");
        let p = units_paragraph();
        assert!(p.contains("long/short.stop_loss_pct"), "{p}");
        assert!(p.contains("oos_split_pct (the fraction of bars kept in-sample; the tail is out-of-sample)"), "{p}");
        assert!(!p.contains('\u{2014}'));
    }

    /// Resolve a manifest unit path in the settings schema: `*` walks into `items`.
    fn resolves(schema: &Value, path: &str) -> bool {
        fn deref_in<'a>(defs: &'a Value, mut s: &'a Value) -> Value {
            for _ in 0..8 {
                if let Some(r) = s.get("$ref").and_then(Value::as_str) {
                    s = &defs[r.trim_start_matches("#/definitions/")];
                    continue;
                }
                if let Some(Value::Array(bs)) = s.get("anyOf").or_else(|| s.get("allOf")) {
                    if let Some(b) = bs.iter().find(|b| b.get("type") != Some(&json!("null"))) {
                        s = b;
                        continue;
                    }
                }
                break;
            }
            s.clone()
        }
        let defs = &schema["definitions"];
        let deref = |s: &Value| deref_in(defs, s);
        let mut cur = deref(schema);
        for seg in path.split('.') {
            let next = if seg == "*" {
                cur.get("items").cloned()
            } else if let Some(p) = cur.pointer(&format!("/properties/{seg}")) {
                Some(p.clone())
            } else {
                // A tagged enum (Sizing): the field lives in one of the branches.
                cur.get("oneOf")
                    .and_then(Value::as_array)
                    .and_then(|bs| bs.iter().find_map(|b| b.pointer(&format!("/properties/{seg}")).cloned()))
            };
            match next {
                Some(n) => cur = deref(&n),
                None => return false,
            }
        }
        true
    }

    #[test]
    fn backtest_manifest_unit_paths_exist_in_the_settings_schema() {
        let schema = crate::mcp::catalog::schema::<Settings>();
        assert!(unit_table().len() > 15, "{:?}", unit_table().keys());
        for path in unit_table().keys() {
            assert!(resolves(&schema, path), "unit path {path} is not a Settings field");
        }
    }

    /// Each hand-listed string vocabulary is exactly what `Settings::validate` accepts.
    #[test]
    fn backtest_manifest_string_vocabularies_match_validate() {
        let base = specs::example_settings();
        let with = |field: &str, v: &str| -> Option<String> {
            let mut s = base.clone();
            match field {
                "kind" => s["kind"] = json!(v),
                "fees.amount_kind" => s["fees"]["amount_kind"] = json!(v),
                "fees.per" => s["fees"]["per"] = json!(v),
                "slippage.kind" => s["slippage"] = json!({"kind": v, "value": 0.0005, "tick_size": 0.01}),
                f if f.contains("stop_loss.kind") => s["long"]["stop_loss"] = json!({"kind": v, "value": 0.02}),
                f if f.contains("trailing_stop.kind") => s["long"]["trailing_stop"] = json!({"kind": v, "value": 0.02}),
                other => panic!("no probe for {other}"),
            }
            if field == "kind" && v != "signals" {
                s["oos_split_pct"] = json!(0);
            }
            let settings: Settings = serde_json::from_value(s).unwrap_or_else(|e| panic!("{field}={v}: {e}"));
            settings.validate()
        };
        for (field, values) in STRING_VOCABS {
            let bogus = with(field, "zzz");
            assert!(bogus.is_some(), "{field}=zzz was accepted");
            for v in *values {
                assert_ne!(with(field, v), bogus, "{field}={v} is refused like an unknown value");
            }
        }
    }

    #[test]
    fn backtest_manifest_metrics_and_positions_match_the_engine() {
        let listed: BTreeSet<&str> = METRIC_INFO.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(listed, METRICS.iter().copied().collect::<BTreeSet<_>>());
        for (name, period, _) in METRIC_INFO {
            let op: crate::backtest::Operand = serde_json::from_value(json!({"kind": "metric", "metric": name, "period": 0})).unwrap();
            assert_eq!(operand_check::check_operand(&op).is_err(), *period, "{name} period requirement");
        }
        let src = include_str!("backtest/dca.rs");
        let at = src.find("const POS_FIELDS").expect("POS_FIELDS in dca.rs");
        let block = &src[at..at + src[at..].find("];").unwrap()];
        let fields: Vec<&str> = block.split('"').skip(1).step_by(2).collect();
        assert_eq!(fields, POSITION_FIELDS);
    }

    #[test]
    fn backtest_manifest_limits_match_the_handlers() {
        let src = include_str!("backtest_api.rs");
        assert!(src.contains(&format!("const MAX_SWEEP_TRIALS: usize = {SWEEP_TRIALS};")));
        assert!(src.contains(&format!("const MAX_SWEEP_AXES: usize = {SWEEP_AXES};")));
        assert!(src.contains(&format!("ids.len() > {MAX_DATASETS}")));
    }

    #[test]
    fn backtest_manifest_compact_form_is_short() {
        let c = compact();
        assert!(c.len() < 3000, "{} chars:\n{c}", c.len());
        assert!(c.contains("(fast=12,slow=26) macd*"), "{c}");
        assert!(c.contains("units: the FRACTIONS / PERCENTS lists of POST /api/backtest/run"));
        assert!(!c.contains('\u{2014}'));
    }
}
