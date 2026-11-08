//! The built-in indicator catalog: one row per name [`super::resolve_builtin`] answers, with the
//! parameters it reads. Every list a caller checks against is derived from this table
//! (`BUILTIN_INDICATORS`, `indicators::is_chainable`, the settings schema), so a name or a
//! parameter cannot be known in one place and unknown in another.

/// One built-in indicator.
pub struct IndicatorSpec {
    pub name: &'static str,
    /// The parameters it reads, each with the default a 0 stands for. A parameter missing here
    /// is ignored by the engine, so a non-zero value for it is refused rather than run.
    pub params: &'static [(&'static str, f64)],
    /// Reads a single series, so a custom-indicator step may chain it onto an earlier step.
    pub chainable: bool,
    /// What the series is, in a few words. For the catalog a caller reads; the engine does not.
    pub output: &'static str,
    /// Reads more than the close (high, low or volume).
    pub needs_ohlc: bool,
    /// Indicators sharing one set of lengths: `macd`, `macd_signal` and `macd_hist` are three
    /// views of one MACD, so a fast length of 12 on each is one parameter, not three. The name
    /// itself when the indicator stands alone.
    pub family: &'static str,
}

const fn row(
    name: &'static str,
    params: &'static [(&'static str, f64)],
    chainable: bool,
    output: &'static str,
    needs_ohlc: bool,
) -> IndicatorSpec {
    IndicatorSpec { name, params, chainable, output, needs_ohlc, family: family_of(name) }
}

/// The family of a built-in: what is left of its name once a view suffix is cut off.
const fn family_of(name: &'static str) -> &'static str {
    const FAMILIES: [&str; 5] = ["macd", "bb", "keltner", "donchian", "stoch"];
    let n = name.as_bytes();
    let mut i = 0;
    while i < FAMILIES.len() {
        let f = FAMILIES[i].as_bytes();
        // `name == f` or `name` starts with `f_`.
        if n.len() >= f.len() && (n.len() == f.len() || n[f.len()] == b'_') {
            let mut j = 0;
            let mut same = true;
            while j < f.len() {
                if n[j] != f[j] {
                    same = false;
                }
                j += 1;
            }
            if same {
                return FAMILIES[i];
            }
        }
        i += 1;
    }
    name
}

const P20: &[(&str, f64)] = &[("period", 20.0)];
const P14: &[(&str, f64)] = &[("period", 14.0)];
const STOCH: &[(&str, f64)] = &[("period", 14.0), ("signal_period", 3.0)];
const MACD: &[(&str, f64)] = &[("fast", 12.0), ("slow", 26.0)];
const MACD_SIG: &[(&str, f64)] = &[("fast", 12.0), ("slow", 26.0), ("signal_period", 9.0)];
const BAND: &[(&str, f64)] = &[("period", 20.0), ("mult", 2.0)];

/// Every built-in, in the order the strategy builder lists them.
pub const INDICATOR_SPECS: &[IndicatorSpec] = &[
    row("sma", P20, true, "simple moving average", false),
    row("ema", P20, true, "exponential moving average", false),
    row("dema", P20, true, "double exponential moving average", false),
    row("tema", P20, true, "triple exponential moving average", false),
    row("wma", P20, true, "weighted moving average", false),
    row("hma", P20, true, "hull moving average", false),
    row("vwap", P20, false, "rolling volume-weighted average price", true),
    row("rsi", P14, true, "relative strength index (0 to 100)", false),
    row("stoch_k", STOCH, false, "stochastic %K (0 to 100)", true),
    row("stoch_d", STOCH, false, "stochastic %D, the smoothed %K (0 to 100)", true),
    row("cci", P20, false, "commodity channel index", true),
    row("willr", P14, false, "williams %R (-100 to 0)", true),
    row("roc", &[("period", 12.0)], true, "rate of change in percent", false),
    row("momentum", &[("period", 10.0)], true, "close minus the close period bars ago", false),
    row("macd", MACD, true, "macd line (fast ema minus slow ema)", false),
    row("macd_signal", MACD_SIG, true, "macd signal line", false),
    row("macd_hist", MACD_SIG, true, "macd histogram (line minus signal)", false),
    row("adx", P14, false, "average directional index (0 to 100)", true),
    row("mfi", P14, false, "money flow index (0 to 100)", true),
    row("obv", &[], false, "on-balance volume", true),
    row("atr", P14, false, "average true range", true),
    row("stddev", P20, true, "standard deviation of the close", false),
    row("bb_upper", BAND, false, "bollinger upper band", false),
    row("bb_mid", P20, false, "bollinger middle band", false),
    row("bb_lower", BAND, false, "bollinger lower band", false),
    row("keltner_upper", BAND, false, "keltner upper band", true),
    row("keltner_lower", BAND, false, "keltner lower band", true),
    row("donchian_upper", P20, false, "donchian upper band (highest high)", true),
    row("donchian_mid", P20, false, "donchian middle band", true),
    row("donchian_lower", P20, false, "donchian lower band (lowest low)", true),
    row("supertrend", &[("period", 10.0), ("mult", 3.0)], false, "supertrend line", true),
    row("psar", &[("mult", 0.02)], false, "parabolic sar (mult is the step)", true),
];

const N: usize = INDICATOR_SPECS.len();

/// The names of [`INDICATOR_SPECS`], in table order.
pub const NAMES: [&str; N] = {
    let mut out = [""; N];
    let mut i = 0;
    while i < N {
        out[i] = INDICATOR_SPECS[i].name;
        i += 1;
    }
    out
};

/// The row of a built-in, exact name only (the engine is case-sensitive).
pub fn spec(name: &str) -> Option<&'static IndicatorSpec> {
    INDICATOR_SPECS.iter().find(|s| s.name == name)
}

/// The built-in a mistyped name most likely meant: the same name in another case, else the
/// closest one within two edits. None when nothing is that close.
pub fn suggest(name: &str) -> Option<&'static str> {
    let key = name.trim().to_ascii_lowercase();
    if let Some(s) = spec(&key) {
        return Some(s.name);
    }
    NAMES
        .iter()
        .map(|n| (edit_distance(&key, n), *n))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

/// Levenshtein distance over bytes (names are ASCII).
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// A complete, runnable strategy: MACD 12/26 crossing its 9-bar signal line to enter, the
/// reverse cross to exit, a 7% stop, 0.05% fees and slippage, the first 70% of the bars
/// in-sample. The manifest and the catalog examples serve it, so a caller composing settings
/// starts from something the engine accepts; a test deserializes and validates it.
pub const EXAMPLE_SETTINGS: &str = r#"{
  "kind": "signals",
  "mode": "long",
  "long": {
    "entry": {"logic": "all", "conditions": [
      {"left": {"kind": "indicator", "indicator": "macd", "fast": 12, "slow": 26},
       "op": "crosses_above",
       "right": {"kind": "indicator", "indicator": "macd_signal", "fast": 12, "slow": 26, "signal_period": 9}}]},
    "exit": {"logic": "all", "conditions": [
      {"left": {"kind": "indicator", "indicator": "macd", "fast": 12, "slow": 26},
       "op": "crosses_below",
       "right": {"kind": "indicator", "indicator": "macd_signal", "fast": 12, "slow": 26, "signal_period": 9}}]},
    "stop_loss_pct": 0.07,
    "exit_on_reverse": false
  },
  "pyramiding": 1,
  "sizing": {"mode": "percent_equity", "percent": 100},
  "starting_capital": 10000,
  "leverage": 1,
  "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.05},
  "slippage": {"kind": "pct", "value": 0.0005},
  "oos_split_pct": 0.7
}"#;

/// [`EXAMPLE_SETTINGS`] as JSON.
pub fn example_settings() -> serde_json::Value {
    serde_json::from_str(EXAMPLE_SETTINGS).expect("EXAMPLE_SETTINGS is JSON")
}

/// Schema of an indicator name: a string out of the catalog, so a schema check refuses a
/// misspelled name before the request is sent.
pub fn indicator_name_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut s = schemars::json_schema!({ "type": "string" });
    s.insert("enum".into(), serde_json::json!(NAMES));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::{indicators, resolve_builtin, Bars};

    fn series(n: usize) -> (Vec<String>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.37).sin() * 6.0 + (i as f64 * 0.05).cos() * 4.0).collect();
        let high: Vec<f64> = close.iter().enumerate().map(|(i, c)| c + 1.0 + (i % 3) as f64 * 0.4).collect();
        let low: Vec<f64> = close.iter().enumerate().map(|(i, c)| c - 1.0 - (i % 4) as f64 * 0.3).collect();
        let vol: Vec<f64> = (0..n).map(|i| 1000.0 + (i % 7) as f64 * 130.0).collect();
        (ts, close, high, low, vol)
    }

    /// The names `resolve_builtin` matches on, read from its source: the table and the match
    /// cannot drift apart without this test failing.
    fn match_arms() -> Vec<String> {
        let src = include_str!("mod.rs");
        let start = src.find("pub(crate) fn resolve_builtin(").expect("resolve_builtin in mod.rs");
        let body = &src[start..];
        let end = body.find("\n}\n").expect("end of resolve_builtin");
        body[..end]
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix('"')?;
                let (name, tail) = rest.split_once('"')?;
                tail.trim_start().starts_with("=>").then(|| name.to_string())
            })
            .collect()
    }

    #[test]
    fn every_match_arm_has_a_spec_and_every_spec_an_arm() {
        let arms = match_arms();
        assert!(arms.len() > 20, "parsed {arms:?}");
        for a in &arms {
            assert!(spec(a).is_some(), "resolve_builtin answers \"{a}\" but INDICATOR_SPECS has no row for it");
        }
        for s in INDICATOR_SPECS {
            assert!(arms.iter().any(|a| a == s.name), "INDICATOR_SPECS lists \"{}\" but resolve_builtin has no arm", s.name);
        }
        let mut seen = std::collections::HashSet::new();
        assert!(INDICATOR_SPECS.iter().all(|s| seen.insert(s.name)), "duplicate row");
    }

    /// The chainable subset is exactly the one custom-indicator steps could chain before the
    /// table existed.
    #[test]
    fn chainable_set_is_unchanged() {
        let before = [
            "sma", "ema", "dema", "tema", "wma", "hma", "rsi", "roc", "momentum", "stddev", "macd", "macd_signal",
            "macd_hist",
        ];
        let mut now: Vec<&str> = INDICATOR_SPECS.iter().filter(|s| s.chainable).map(|s| s.name).collect();
        let mut before = before.to_vec();
        now.sort_unstable();
        before.sort_unstable();
        assert_eq!(now, before);
        for s in INDICATOR_SPECS {
            assert_eq!(indicators::is_chainable(s.name), s.chainable, "{}", s.name);
        }
        assert!(!indicators::is_chainable("nope"));
    }

    /// A listed parameter moves the series, an unlisted one does not: the table says what the
    /// engine really reads, on the candles and on a chained series alike.
    #[test]
    fn listed_params_are_exactly_the_ones_read() {
        let (ts, close, high, low, vol) = series(400);
        let b = Bars { ticker: "", ts: &ts, open: &close, high: &high, low: &low, close: &close, volume: &vol, quotes: None, sub: None, script: None };
        let src: Vec<Option<f64>> = close.iter().map(|c| Some(*c)).collect();
        // (param, value) far enough from every default to move any reader.
        let probes: [(&str, usize, usize, usize, f64, usize); 5] = [
            ("period", 7, 0, 0, 0.0, 0),
            ("fast", 0, 5, 0, 0.0, 0),
            ("slow", 0, 0, 40, 0.0, 0),
            ("mult", 0, 0, 0, 1.3, 0),
            ("signal_period", 0, 0, 0, 0.0, 5),
        ];
        for s in INDICATOR_SPECS {
            let base = resolve_builtin(s.name, 0, 0, 0, 0.0, 0, &b);
            let chained = s.chainable.then(|| indicators::resolve_chainable(s.name, &src, 0, 0, 0, 0));
            for (key, p, f, sl, m, sp) in probes {
                let listed = s.params.iter().any(|(k, _)| *k == key);
                let moved = resolve_builtin(s.name, p, f, sl, m, sp, &b) != base;
                assert_eq!(moved, listed, "{} param {key}: listed {listed}, read {moved}", s.name);
                if let Some(c) = &chained {
                    if key != "mult" {
                        let moved = indicators::resolve_chainable(s.name, &src, p, f, sl, sp) != *c;
                        assert_eq!(moved, listed, "{} chained param {key}", s.name);
                    }
                }
            }
            // The default each row states is the value a 0 stands for.
            for (key, def) in s.params {
                let d = *def;
                let at = |v: f64| match *key {
                    "period" => resolve_builtin(s.name, v as usize, 0, 0, 0.0, 0, &b),
                    "fast" => resolve_builtin(s.name, 0, v as usize, 0, 0.0, 0, &b),
                    "slow" => resolve_builtin(s.name, 0, 0, v as usize, 0.0, 0, &b),
                    "mult" => resolve_builtin(s.name, 0, 0, 0, v, 0, &b),
                    _ => resolve_builtin(s.name, 0, 0, 0, 0.0, v as usize, &b),
                };
                assert_eq!(at(d), base, "{} default of {key}", s.name);
            }
        }
    }

    #[test]
    fn views_of_one_indicator_share_a_family() {
        let fam = |n: &str| spec(n).map(|s| s.family);
        assert_eq!(fam("macd"), Some("macd"));
        assert_eq!(fam("macd_signal"), Some("macd"));
        assert_eq!(fam("macd_hist"), Some("macd"));
        assert_eq!(fam("bb_mid"), Some("bb"));
        assert_eq!(fam("keltner_lower"), Some("keltner"));
        assert_eq!(fam("donchian_upper"), Some("donchian"));
        assert_eq!(fam("stoch_d"), Some("stoch"));
        assert_eq!(fam("sma"), Some("sma"));
        assert_eq!(fam("stddev"), Some("stddev"));
    }

    #[test]
    fn suggestions_fix_case_then_small_typos() {
        assert_eq!(suggest("SMA"), Some("sma"));
        assert_eq!(suggest(" Ema "), Some("ema"));
        assert_eq!(suggest("smaa"), Some("sma"));
        assert_eq!(suggest("macd_signl"), Some("macd_signal"));
        assert_eq!(suggest("bollinger"), None);
        assert_eq!(spec("SMA").map(|s| s.name), None);
    }

    /// The example everyone is pointed at must run as-is.
    #[test]
    fn example_settings_deserialize_and_validate() {
        let s: crate::backtest::Settings = serde_json::from_str(EXAMPLE_SETTINGS).expect("deserializes");
        assert_eq!(s.validate(), None);
        assert_eq!(s.oos_split_pct, 0.7);
        let v = example_settings();
        assert_eq!(v["long"]["entry"]["conditions"][0]["left"]["indicator"], "macd");
    }

    #[test]
    fn the_schema_lists_every_name() {
        let mut g = schemars::SchemaGenerator::default();
        let s = indicator_name_schema(&mut g);
        let names = s.get("enum").and_then(|v| v.as_array()).expect("enum");
        assert_eq!(names.len(), INDICATOR_SPECS.len());
        assert_eq!(names[0], "sma");
    }
}
