//! Host layer: timeframe parsing, close times, JSON glue, registry export, bar validation.

mod common;

use common::*;
use otw_script::diag::RtCode;
use otw_script::host::{self, BarCols};
use otw_script::input::{Limits, TfUnit, Timeframe};
use otw_script::output::Rgba;
use serde_json::Value as Json;

fn tf(multiplier: u32, unit: TfUnit) -> Option<Timeframe> {
    Some(Timeframe { multiplier, unit })
}

#[test]
fn timeframe_parse() {
    assert_eq!(Timeframe::parse("1m"), tf(1, TfUnit::Minute));
    assert_eq!(Timeframe::parse("4h"), tf(240, TfUnit::Minute));
    assert_eq!(Timeframe::parse("1d"), tf(1, TfUnit::Day));
    assert_eq!(Timeframe::parse("1w"), tf(1, TfUnit::Week));
    assert_eq!(Timeframe::parse("1M"), tf(1, TfUnit::Month));
    assert_eq!(Timeframe::parse("1mo"), tf(1, TfUnit::Month));
    assert_eq!(Timeframe::parse("30s"), tf(30, TfUnit::Second));
    for bad in ["", "0m", "h", "1x"] {
        assert_eq!(Timeframe::parse(bad), None, "{bad}");
    }
}

#[test]
fn close_times() {
    let h = Timeframe { multiplier: 1, unit: TfUnit::Minute };
    let hour = Timeframe { multiplier: 60, ..h };
    assert_eq!(host::close_times(&[0, 3_600_000], hour), vec![3_600_000, 7_200_000]);
    let month = Timeframe { multiplier: 1, unit: TfUnit::Month };
    // 2024-01-01, 2024-12-01 → 2024-02-01, 2025-01-01 (UTC ms).
    assert_eq!(host::close_times(&[1_704_067_200_000, 1_733_011_200_000], month), vec![1_706_745_600_000, 1_735_689_600_000]);
}

#[test]
fn default_plot_color() {
    let out = run_src(&v6("plot(close)"), bars_close(&[1.0, 2.0]));
    assert_eq!(out.plots[0].colors[0], Some(Rgba { r: 0x29, g: 0x62, b: 0xFF, a: 255 }));
    let out = run_src(&v6("plot(close, color=na)"), bars_close(&[1.0, 2.0]));
    assert_eq!(out.plots[0].colors[0], None);
    let out = run_src(&v6("hline(1)"), bars_close(&[1.0]));
    assert_eq!(out.hlines[0].color, Some(Rgba { r: 0x78, g: 0x7B, b: 0x86, a: 255 }));
}

#[test]
fn volume_unknown() {
    let mut b = bars_close(&[1.0, 2.0, 3.0]);
    b.volume.clear();
    let ok = compile(&v6("plot(close)")).run(&input(b.clone()));
    assert!(ok.is_ok(), "{ok:?}");
    let e = compile(&v6("plot(volume)")).run(&input(b.clone())).unwrap_err();
    assert_eq!((e.code, e.bar), (RtCode::VolumeUnknown, 0));
    let e = compile(&v6("plot(ta.vwma(close, 3))")).run(&input(b)).unwrap_err();
    assert_eq!(e.code, RtCode::VolumeUnknown);
}

#[test]
fn bad_bars() {
    let mut b = bars_close(&[1.0, 2.0, 3.0]);
    b.open.pop();
    let e = compile(&v6("plot(close)")).run(&input(b)).unwrap_err();
    assert_eq!((e.code, e.args), (RtCode::BadBars, vec!["open".to_string()]));
}

fn ctx_json() -> String {
    serde_json::json!({ "syminfo": syminfo(), "timeframe": "1h", "inputs": {} }).to_string()
}

fn cols<'a>(t: &'a [f64], c: &'a [f64]) -> BarCols<'a> {
    BarCols { time: t, open: c, high: c, low: c, close: c, volume: c }
}

#[test]
fn run_json_roundtrip() {
    let p = compile(&v6("plot(close)"));
    let (t, c) = ([0.0, 3_600_000.0], [1.0, 2.0]);
    let s = host::run_json(&p, cols(&t, &c), &ctx_json(), Limits::default());
    let j: Json = serde_json::from_str(&s).unwrap();
    assert_eq!(j["ok"], true, "{s}");
    assert_eq!(j["output"]["plots"].as_array().unwrap().len(), 1);
}

#[test]
fn bad_context() {
    let p = compile(&v6("plot(close)"));
    let (t, c) = ([0.0], [1.0]);
    let s = host::run_json(&p, cols(&t, &c), "{", Limits::default());
    let j: Json = serde_json::from_str(&s).unwrap();
    assert_eq!(j["ok"], false);
    assert!(j["context"].is_string(), "{s}");
}

#[test]
fn compile_json_shapes() {
    let j: Json = serde_json::from_str(&host::compile_json(&v6("plot(close)"))).unwrap();
    assert_eq!(j["ok"], true);
    assert_eq!(j["meta"]["plots"][0]["title"], "Plot");
    assert!(j["warnings"].is_array());
    let j: Json = serde_json::from_str(&host::compile_json(&v6("plot(nope)"))).unwrap();
    assert_eq!(j["ok"], false);
    assert_eq!(j["diagnostics"][0]["code"], "UnknownName");
}

#[test]
fn registry_docs() {
    let docs = otw_script::registry::docs();
    let sma = docs.iter().find(|d| d.name == "ta.sma").expect("ta.sma");
    assert!(sma.supported);
    assert_eq!(sma.params.iter().map(|p| p.name).collect::<Vec<_>>(), ["source", "length"]);
    let mut seen = std::collections::BTreeSet::new();
    for d in &docs {
        assert!(seen.insert((d.name, d.kind)), "duplicate {} {}", d.name, d.kind);
    }
}


/// The same cases `frontend/tests/script-host.test.js` runs through `inputValues`.
#[test]
fn input_values_fixture() {
    use otw_script::output::InputMeta;
    let cases: Vec<Json> = serde_json::from_str(include_str!("fixtures/input_values.json")).unwrap();
    assert!(cases.len() >= 15);
    for c in cases {
        let meta: InputMeta = serde_json::from_value(c["meta"].clone()).unwrap();
        let mut raw = serde_json::Map::new();
        raw.insert(meta.key.clone(), c["raw"].clone());
        let (values, errors) = host::input_values(std::slice::from_ref(&meta), &raw);
        match c["expect"].get("reason") {
            Some(reason) => {
                assert!(values.is_empty(), "{c}");
                assert_eq!(errors.len(), 1, "{c}");
                assert_eq!((errors[0].key.as_str(), errors[0].reason), (meta.key.as_str(), reason.as_str().unwrap()), "{c}");
            }
            None => {
                assert!(errors.is_empty(), "{c}: {:?}", errors.first().map(|e| e.reason));
                assert_eq!(serde_json::to_value(&values[&meta.key]).unwrap(), c["expect"], "{c}");
            }
        }
    }
    // Absent keys are left to the script's default.
    let meta: InputMeta = serde_json::from_value(serde_json::json!({"key":"len","title":"L","kind":"Int","default":{"Int":1},"minval":null,"maxval":null,"step":null,"options":[]})).unwrap();
    let (values, errors) = host::input_values(&[meta], &serde_json::Map::new());
    assert!(values.is_empty() && errors.is_empty());
}

#[test]
fn first_bar_window() {
    assert_eq!(host::FIRST_BAR_WINDOW, 5_000);
}
