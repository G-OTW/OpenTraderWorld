//! Why an operand cannot be read, decided without bars or the library. Shared by every caller
//! that takes a signal operand from outside: strategy settings, chart alerts, scanner signals
//! and the journal lab. An unknown name used to resolve to a series undefined on every bar, so
//! the condition never held and nothing said why.

use super::specs::{self, IndicatorSpec};
use super::Operand;

/// Longest length an indicator or a metric may ask for.
pub const MAX_PERIOD: usize = 500;

/// The base check every caller applies: known names, lengths in range, and no portfolio state
/// (only a DCA run has one).
pub fn check_operand(op: &Operand) -> Result<(), String> {
    match op {
        Operand::Const { value } if !value.is_finite() => Err("value must be a number".into()),
        Operand::Indicator { indicator, period, fast, slow, signal_period, mult } => {
            known_indicator(indicator)?;
            check_lengths(&[*period, *fast, *slow, *signal_period], *mult)
        }
        Operand::Metric { metric, period } => {
            if !super::METRICS.contains(&metric.as_str()) {
                return Err(format!("unknown metric \"{metric}\". Valid: {}", super::METRICS.join(", ")));
            }
            if ["change_pct", "rel_volume"].contains(&metric.as_str()) && !(1..=MAX_PERIOD).contains(period) {
                return Err(format!("the metric period must be between 1 and {MAX_PERIOD} bars"));
            }
            Ok(())
        }
        Operand::Position { .. } => Err("portfolio state is only known inside a DCA run".into()),
        _ => Ok(()),
    }
}

/// The spec of a built-in, or the refusal naming the closest one and the whole list.
pub fn known_indicator(name: &str) -> Result<&'static IndicatorSpec, String> {
    if let Some(s) = specs::spec(name) {
        return Ok(s);
    }
    let hint = specs::suggest(name).map(|s| format!(" (did you mean \"{s}\"?)")).unwrap_or_default();
    Err(format!("unknown indicator \"{name}\"{hint}. Valid: {}", specs::NAMES.join(", ")))
}

fn check_lengths(lengths: &[usize], mult: f64) -> Result<(), String> {
    if lengths.iter().any(|p| *p > MAX_PERIOD) || !mult.is_finite() {
        return Err(format!("indicator lengths go up to {MAX_PERIOD} bars"));
    }
    Ok(())
}

/// What a strategy adds on top of [`check_operand`] for a built-in: every non-zero parameter
/// must be one the indicator reads (the engine ignores the others), and a multiplier cannot be
/// negative (it used to fall back to the default without a word).
pub fn check_params(
    spec: &IndicatorSpec,
    period: usize,
    fast: usize,
    slow: usize,
    mult: f64,
    signal_period: usize,
) -> Result<(), String> {
    let given = [
        ("period", period as f64),
        ("fast", fast as f64),
        ("slow", slow as f64),
        ("mult", mult),
        ("signal_period", signal_period as f64),
    ];
    let reads = |k: &str| spec.params.iter().any(|(p, _)| *p == k);
    if let Some((key, _)) = given.iter().find(|(k, v)| *v != 0.0 && !reads(k)) {
        let list: Vec<&str> = spec.params.iter().map(|(p, _)| *p).collect();
        let what = if list.is_empty() { "no parameter".to_string() } else { list.join(", ") };
        let hint = match (spec.name, *key) {
            ("macd", "signal_period") => "; use \"macd_signal\" for the signal line",
            ("bb_mid", "mult") => "; the middle band is the average, only the outer bands read mult",
            _ => "",
        };
        return Err(format!("\"{}\" does not read {key} (it reads {what}){hint}", spec.name));
    }
    if mult < 0.0 {
        let def = spec.params.iter().find(|(p, _)| *p == "mult").map(|(_, d)| *d).unwrap_or(0.0);
        return Err(format!("\"{}\" mult cannot be negative (0 = the default {def})", spec.name));
    }
    Ok(())
}

/// The full check of a built-in a strategy reads: [`check_operand`] then [`check_params`].
pub fn check_indicator(
    name: &str,
    period: usize,
    fast: usize,
    slow: usize,
    mult: f64,
    signal_period: usize,
) -> Result<&'static IndicatorSpec, String> {
    let spec = known_indicator(name)?;
    check_lengths(&[period, fast, slow, signal_period], mult)?;
    check_params(spec, period, fast, slow, mult, signal_period)?;
    Ok(spec)
}

/// A price field named by a custom-indicator step (stored as text there).
pub fn check_price_field(field: &str) -> Result<(), String> {
    if super::PriceField::ALL.iter().any(|f| f.as_str() == field) {
        return Ok(());
    }
    Err(format!("price field must be open, high, low, close or volume (got \"{field}\")"))
}

#[cfg(test)]
mod tests {
    use super::super::{AfterAddSl, Logic, PriceField, Settings, Sizing, TierMetric};
    use serde_json::{json, Value};

    fn ind(name: &str) -> Value {
        json!({"kind": "indicator", "indicator": name, "period": 10})
    }

    fn with_entry(cond: Value) -> Value {
        json!({
            "mode": "long",
            "sizing": {"mode": "percent_equity", "percent": 10},
            "long": {"entry": {"logic": "all", "conditions": [cond]}},
        })
    }

    fn cross(left: Value, right: Value) -> Value {
        json!({"left": left, "op": "crosses_above", "right": right})
    }

    fn parsed(v: Value) -> Settings {
        serde_json::from_value(v).expect("settings parse")
    }

    fn refused(v: Value) -> String {
        parsed(v).validate().expect("settings refused")
    }

    fn parse_error(v: Value) -> String {
        serde_json::from_value::<Settings>(v).err().expect("parse refused").to_string()
    }

    #[test]
    fn a_wrong_case_indicator_is_refused_with_the_right_name() {
        let e = refused(with_entry(cross(ind("SMA"), ind("ema"))));
        assert!(
            e.starts_with("long.entry.conditions[0].left: unknown indicator \"SMA\" (did you mean \"sma\"?). Valid: sma, ema, dema"),
            "{e}"
        );
    }

    #[test]
    fn a_misspelled_indicator_is_refused_with_a_suggestion() {
        let e = refused(with_entry(cross(ind("ema"), ind("smaa"))));
        assert!(e.starts_with("long.entry.conditions[0].right: unknown indicator \"smaa\" (did you mean \"sma\"?)"), "{e}");
        let e = refused(with_entry(cross(ind("bollinger"), ind("ema"))));
        assert!(e.contains("unknown indicator \"bollinger\". Valid: sma"), "{e}");
    }

    #[test]
    fn the_lowercase_names_still_validate() {
        assert_eq!(parsed(with_entry(cross(ind("sma"), ind("ema")))).validate(), None);
    }

    #[test]
    fn logic_reads_or_as_any_and_refuses_the_rest() {
        let mut v = with_entry(cross(ind("sma"), ind("ema")));
        for (word, want) in [("or", Logic::Any), ("ANY ", Logic::Any), ("and", Logic::All), ("All", Logic::All)] {
            v["long"]["entry"]["logic"] = json!(word);
            assert_eq!(parsed(v.clone()).long.unwrap().entry.unwrap().logic, want, "{word}");
        }
        v["long"]["entry"]["logic"] = json!("xor");
        let e = parse_error(v);
        assert!(e.contains("unknown logic \"xor\", expected one of: all, any, and, or"), "{e}");
    }

    #[test]
    fn a_price_field_reads_any_case_and_refuses_unknown_fields() {
        let v = with_entry(cross(json!({"kind": "price", "field": "Close"}), ind("sma")));
        let s = parsed(v);
        let left = &s.long.as_ref().unwrap().entry.as_ref().unwrap().conditions[0].left;
        assert!(matches!(left, super::super::Operand::Price { field: PriceField::Close }));
        let e = parse_error(with_entry(cross(json!({"kind": "price", "field": "hl2"}), ind("sma"))));
        assert!(e.contains("unknown price field \"hl2\", expected one of: open, high, low, close, volume"), "{e}");
    }

    #[test]
    fn an_unknown_tier_metric_is_refused() {
        let mut v = with_entry(cross(ind("sma"), ind("ema")));
        v["sizing"] = json!({"mode": "equity_tiers", "metric": "risk", "tiers": [{"above": 0, "value": 2}]});
        let e = parse_error(v.clone());
        assert!(e.contains("unknown equity-tier metric \"risk\", expected one of: qty, risk_pct, percent_equity"), "{e}");
        v["sizing"]["metric"] = json!("Percent_Equity");
        assert!(matches!(parsed(v).sizing, Sizing::EquityTiers { metric: TierMetric::PercentEquity, .. }));
    }

    #[test]
    fn an_unknown_after_add_stop_is_refused() {
        let mut v = with_entry(cross(ind("sma"), ind("ema")));
        v["pyramid_steps"] = json!({"after_add_sl": "trail"});
        let e = parse_error(v.clone());
        assert!(e.contains("unknown after_add_sl \"trail\", expected one of: none, breakeven, trail_avg"), "{e}");
        v["pyramid_steps"] = json!({"after_add_sl": "trail_avg"});
        assert_eq!(parsed(v).pyramid_steps.after_add_sl, AfterAddSl::TrailAvg);
    }

    #[test]
    fn a_negative_mult_is_refused() {
        let bb = json!({"kind": "indicator", "indicator": "bb_upper", "period": 20, "mult": -2.0});
        let e = refused(with_entry(cross(json!({"kind": "price", "field": "close"}), bb)));
        assert_eq!(e, "long.entry.conditions[0].right: \"bb_upper\" mult cannot be negative (0 = the default 2)");
    }

    #[test]
    fn a_parameter_the_indicator_ignores_is_refused() {
        let macd = json!({"kind": "indicator", "indicator": "macd", "fast": 12, "slow": 26, "signal_period": 9});
        let e = refused(with_entry(cross(macd, json!({"kind": "const", "value": 0}))));
        assert_eq!(
            e,
            "long.entry.conditions[0].left: \"macd\" does not read signal_period (it reads fast, slow); use \"macd_signal\" for the signal line"
        );
        let sma = json!({"kind": "indicator", "indicator": "sma", "period": 20, "fast": 5});
        let e = refused(with_entry(cross(sma, ind("ema"))));
        assert!(e.ends_with("\"sma\" does not read fast (it reads period)"), "{e}");
        let obv = json!({"kind": "indicator", "indicator": "obv", "period": 20});
        let e = refused(with_entry(cross(obv, ind("ema"))));
        assert!(e.ends_with("\"obv\" does not read period (it reads no parameter)"), "{e}");
    }

    #[test]
    fn a_length_past_the_ceiling_is_refused() {
        let long = json!({"kind": "indicator", "indicator": "sma", "period": 501});
        let e = refused(with_entry(cross(long, ind("ema"))));
        assert_eq!(e, "long.entry.conditions[0].left: indicator lengths go up to 500 bars");
    }

    #[test]
    fn a_comparison_without_a_right_operand_is_refused() {
        let e = refused(with_entry(json!({"left": ind("sma"), "op": "above"})));
        assert_eq!(e, "long.entry.conditions[0]: \"above\" compares two series: give it a right operand");
        assert_eq!(parsed(with_entry(json!({"left": ind("sma"), "op": "rising"}))).validate(), None);
    }

    #[test]
    fn portfolio_state_is_refused_outside_a_dca_plan() {
        let pos = json!({"kind": "position", "field": "pnl_pct"});
        let e = refused(with_entry(cross(pos, json!({"kind": "const", "value": 5}))));
        assert_eq!(e, "long.entry.conditions[0].left: portfolio state is only known inside a DCA run");
    }

    #[test]
    fn a_plot_on_a_node_graph_indicator_is_refused() {
        let mut v = with_entry(cross(json!({"kind": "custom_indicator", "id": "osc", "plot": "main"}), json!({"kind": "const", "value": 0})));
        v["indicators"] = json!({"osc": {"nodes": [{"op": "price", "field": "close"}, {"op": "indicator", "indicator": "rsi", "period": 14, "src": 0}], "output": 1}});
        let e = refused(v.clone());
        assert!(e.starts_with("long.entry.conditions[0].left: custom indicator \"osc\" is a node graph with a single output: drop plot \"main\""), "{e}");
        v["long"]["entry"]["conditions"][0]["left"] = json!({"kind": "custom_indicator", "id": "osc"});
        assert_eq!(parsed(v).validate(), None);
    }

    #[test]
    fn a_node_graph_step_is_checked_like_an_operand() {
        let mut v = with_entry(cross(json!({"kind": "custom_indicator", "id": "osc"}), json!({"kind": "const", "value": 0})));
        v["indicators"] = json!({"osc": {"nodes": [{"op": "price", "field": "close"}, {"op": "indicator", "indicator": "RSI", "period": 14}], "output": 1}});
        let e = refused(v.clone());
        assert!(e.starts_with("indicators.osc.nodes[1]: unknown indicator \"RSI\" (did you mean \"rsi\"?)"), "{e}");
        v["indicators"]["osc"]["nodes"][1] = json!({"op": "indicator", "indicator": "atr", "period": 14, "src": 0});
        let e = refused(v.clone());
        assert!(e.starts_with("indicators.osc.nodes[1]: \"atr\" reads high, low or volume and cannot take another step as its source (only sma,"), "{e}");
        v["indicators"]["osc"]["nodes"][0] = json!({"op": "price", "field": "Close"});
        v["indicators"]["osc"]["nodes"][1] = json!({"op": "indicator", "indicator": "rsi", "period": 14});
        let e = refused(v);
        assert_eq!(e, "indicators.osc.nodes[0]: price field must be open, high, low, close or volume (got \"Close\")");
    }

    #[test]
    fn dca_rules_are_walked_and_read_every_metric() {
        let rule = |left: Value| {
            json!({
                "kind": "dca",
                "mode": "long",
                "sizing": {"mode": "percent_equity", "percent": 10},
                "starting_capital": 1000,
                "dca": {"buys": [{"amount": 100, "condition": {"conditions": [
                    {"left": left, "op": "above", "right": {"kind": "const", "value": 1.5}}
                ]}}]},
            })
        };
        assert_eq!(parsed(rule(json!({"kind": "metric", "metric": "rel_volume", "period": 20}))).validate(), None);
        assert_eq!(parsed(rule(json!({"kind": "position", "field": "pnl_pct"}))).validate(), None);
        let e = refused(rule(ind("Ema")));
        assert!(e.starts_with("dca.buys[0].condition.conditions[0].left: unknown indicator \"Ema\" (did you mean \"ema\"?)"), "{e}");
    }

    #[test]
    fn the_shared_check_keeps_accepting_what_alerts_accepted() {
        // Alerts, the scanner and the journal lab read the base check: an unused parameter is
        // still theirs to ignore, an unknown name still refused.
        let op: super::super::Operand = serde_json::from_value(json!({"kind": "indicator", "indicator": "sma", "period": 20, "fast": 5})).unwrap();
        assert_eq!(super::check_operand(&op), Ok(()));
        let op: super::super::Operand = serde_json::from_value(ind("SMA")).unwrap();
        assert!(super::check_operand(&op).unwrap_err().contains("unknown indicator \"SMA\""));
    }
}
