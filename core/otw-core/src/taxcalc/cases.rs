//! The regime test suite: one runner for every country.
//!
//! Adding a country adds files, never test code:
//!
//!   - every `regimes/<id>/<year>.toml` must be listed in `regime::FILES`, load, validate,
//!     name at least one source, and have a `<year>.cases.toml` beside it with at least
//!     two `[[case]]`;
//!   - every case names its source and states the expected total (and, optionally, pool
//!     figures); every `[[lots_case]]` runs fills through the regime's cost method and
//!     anti-wash rule and checks the realized total;
//!   - invariants hold for every regime on generated inputs: tax is finite, never negative,
//!     never above the positive amounts, and never falls when a gain grows.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::lots::{self, Fill, Valuation};
use super::regime::{self, Regime};

const TOL: f64 = 0.01;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/taxcalc/regimes")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFile {
    #[serde(default)]
    case: Vec<Case>,
    #[serde(default)]
    lots_case: Vec<LotsCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    source: String,
    #[serde(default = "itemized")]
    mode: String,
    #[serde(default = "investing")]
    context: String,
    inputs: toml::Value,
    #[serde(default)]
    profile: Option<toml::Value>,
    expect: Expect,
}

fn itemized() -> String {
    "itemized".into()
}
fn investing() -> String {
    "investing".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    total_tax: f64,
    /// pool key → { field: value } over the result's pool report.
    #[serde(default)]
    pools: Option<toml::Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LotsCase {
    name: String,
    source: String,
    fills: Vec<CaseFill>,
    expect: LotsExpect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFill {
    at: String,
    side: String,
    qty: f64,
    price: f64,
    #[serde(default = "one")]
    rate: f64,
    #[serde(default = "stock")]
    class: String,
}

fn one() -> f64 {
    1.0
}
fn stock() -> String {
    "stock".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LotsExpect {
    total_gain: f64,
    #[serde(default)]
    disallowed: f64,
}

fn at(s: &str) -> OffsetDateTime {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()
        .or_else(|| regime::parse_ymd(s).map(|d| d.with_time(time::Time::from_hms(12, 0, 0).unwrap()).assume_utc()))
        .unwrap_or_else(|| panic!("{s} is not a date"))
}

fn regimes() -> Vec<Regime> {
    regime::FILES
        .iter()
        .map(|(p, src)| regime::load(p, src).unwrap_or_else(|e| panic!("{p}: {e}")))
        .collect()
}

fn cases_of(r: &Regime) -> CaseFile {
    let path = dir().join(format!("{}/{}.cases.toml", r.id, r.year));
    let src = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} has no cases file", path.display()));
    toml::from_str(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every regime file on disk is compiled in, loads and validates.
#[test]
fn every_file_is_registered_and_valid() {
    let listed: HashSet<&str> = regime::FILES.iter().map(|(p, _)| *p).collect();
    for entry in std::fs::read_dir(dir()).unwrap() {
        let sub = entry.unwrap().path();
        if !sub.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&sub).unwrap() {
            let f = f.unwrap().path();
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".cases.toml") || !name.ends_with(".toml") {
                continue;
            }
            let rel = format!("{}/{name}", sub.file_name().unwrap().to_string_lossy());
            assert!(listed.contains(rel.as_str()), "{rel} is not in regime::FILES");
        }
    }
    let rs = regimes();
    assert_eq!(rs.len(), regime::FILES.len());
}

/// The gate: a source per file, at least two cases, a source per case.
#[test]
fn every_regime_has_sources_and_cases() {
    for r in regimes() {
        let c = cases_of(&r);
        assert!(c.case.len() >= 2, "{} {}: at least two cases", r.id, r.year);
        for k in &c.case {
            assert!(!k.source.trim().is_empty(), "{} {}: case '{}' has no source", r.id, r.year, k.name);
        }
        for k in &c.lots_case {
            assert!(!k.source.trim().is_empty(), "{} {}: lots case '{}' has no source", r.id, r.year, k.name);
        }
    }
}

/// Every case of every regime.
#[test]
fn every_case_passes() {
    let mut failures = Vec::new();
    for r in regimes() {
        for k in cases_of(&r).case {
            let mut profile = json!({ "regime": r.id, "currency": r.currency });
            if let Some(p) = &k.profile {
                for (key, v) in serde_json::to_value(p).unwrap().as_object().unwrap() {
                    profile[key] = v.clone();
                }
            }
            let scenario = json!({
                "mode": k.mode, "context": k.context, "tax_year": r.year,
                "inputs": serde_json::to_value(&k.inputs).unwrap(),
            });
            let out = super::compute(&profile, &scenario);
            let got = out["total_tax"].as_f64().unwrap();
            let tag = format!("{} {} '{}'", r.id, r.year, k.name);
            if (got - k.expect.total_tax).abs() > TOL {
                failures.push(format!("{tag}: total_tax {got}, expected {}", k.expect.total_tax));
            }
            if let Some(w) = out["warnings"].as_array().and_then(|w| {
                w.iter().find(|m| m.as_str().is_some_and(|m| m.contains("not an item") || m.starts_with("No ")))
            }) {
                failures.push(format!("{tag}: {w}"));
            }
            for (pool, fields) in k.expect.pools.iter().flatten() {
                let report = out["pools"].as_array().unwrap().iter().find(|p| p["pool"] == pool.as_str());
                let Some(report) = report else {
                    failures.push(format!("{tag}: no pool {pool}"));
                    continue;
                };
                for (field, want) in fields.as_table().unwrap() {
                    let want = want.as_float().or_else(|| want.as_integer().map(|i| i as f64)).unwrap();
                    let got = report[field].as_f64().unwrap_or(f64::NAN);
                    if (got - want).abs() > TOL {
                        failures.push(format!("{tag}: pool {pool}.{field} {got}, expected {want}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Every lots case: the regime's method and anti-wash rule on real fills.
#[test]
fn every_lots_case_passes() {
    let mut failures = Vec::new();
    for r in regimes() {
        let wash = super::wash_rule(&r);
        for k in cases_of(&r).lots_case {
            let fills = k
                .fills
                .iter()
                .enumerate()
                .map(|(i, f)| Fill {
                    id: format!("{i:04}"),
                    symbol: "X".into(),
                    asset_class: f.class.clone(),
                    bucket: "capital",
                    valuation: Valuation::Spot,
                    at: at(&f.at),
                    buy: f.side == "buy",
                    qty: f.qty,
                    gross: f.qty * f.price,
                    fee: 0.0,
                    currency: r.currency.clone(),
                    rate: Some(f.rate),
                })
                .collect();
            let out = lots::match_fills(fills, r.method(), false, wash.as_ref());
            let tag = format!("{} {} '{}'", r.id, r.year, k.name);
            if !out.errors.is_empty() {
                failures.push(format!("{tag}: {:?}", out.errors));
            }
            let gain: f64 = out.disposals.iter().map(|d| d.gain).sum();
            let dis: f64 = out.disposals.iter().map(|d| d.disallowed).sum();
            if (gain - k.expect.total_gain).abs() > TOL {
                failures.push(format!("{tag}: total_gain {gain}, expected {}", k.expect.total_gain));
            }
            if (dis - k.expect.disallowed).abs() > TOL {
                failures.push(format!("{tag}: disallowed {dis}, expected {}", k.expect.disallowed));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A small deterministic generator: the invariants run on the same inputs every time.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

fn scenario(year: i32, inputs: Value) -> Value {
    json!({ "mode": "itemized", "context": "investing", "tax_year": year, "inputs": inputs })
}

/// For every regime: finite, never negative, never above the positive amounts, and a
/// larger gain never lowers the tax.
#[test]
fn invariants_hold_for_every_regime() {
    let mut g = Lcg(42);
    for r in regimes() {
        let profile = json!({ "regime": r.id });
        for _ in 0..300 {
            let cap = g.range(-50_000.0, 200_000.0);
            let inputs = json!({
                "realized_capital_gains": cap,
                "derivative_gains": g.range(-20_000.0, 50_000.0),
                "crypto_gains": g.range(-20_000.0, 50_000.0),
                "dividends": g.range(0.0, 20_000.0),
                "interest_income": g.range(0.0, 10_000.0),
                "other_income": g.range(0.0, 300_000.0),
                "marginal_rate": g.range(0.0, 45.0),
                "long_term": { "capital": cap * g.next(), "crypto": 0.0 },
            });
            let out = super::compute(&profile, &scenario(r.year, inputs.clone()));
            let tax = out["total_tax"].as_f64().unwrap();
            let positive: f64 = ["realized_capital_gains", "derivative_gains", "crypto_gains", "dividends", "interest_income"]
                .iter()
                .map(|k| inputs[k].as_f64().unwrap().max(0.0))
                .sum();
            assert!(tax.is_finite(), "{} {}: tax not finite", r.id, r.year);
            assert!(tax >= -1e-9, "{} {}: negative tax {tax} for {inputs}", r.id, r.year);
            assert!(tax <= positive + 1e-6, "{} {}: tax {tax} above the amounts {positive}", r.id, r.year);

            let mut more = inputs.clone();
            let bump = g.range(1.0, 10_000.0);
            more["realized_capital_gains"] = json!(cap + bump);
            let tax2 = super::compute(&profile, &scenario(r.year, more.clone()))["total_tax"].as_f64().unwrap();
            assert!(tax2 >= tax - 1e-6, "{} {}: tax fell from {tax} to {tax2} when the gain grew\n{inputs}\n{more}", r.id, r.year);
        }
    }
}

/// Lots: once every position is closed, average and FIFO realize the same total, and the
/// disposed quantity is what was sold.
#[test]
fn lot_methods_agree_on_closed_positions() {
    let mut g = Lcg(7);
    for round in 0..200 {
        let mut fills = Vec::new();
        let mut held = 0.0;
        let mut sold = 0.0;
        let t0 = at("2024-01-01");
        for i in 0..12 {
            let buy = held < 1.0 || g.next() < 0.55;
            let qty = if buy { (g.range(1.0, 20.0)).round() } else { (held * g.next()).round().max(1.0).min(held) };
            if buy {
                held += qty;
            } else {
                held -= qty;
                sold += qty;
            }
            fills.push((i, buy, qty, g.range(10.0, 200.0)));
        }
        if held > 0.0 {
            fills.push((12, false, held, g.range(10.0, 200.0)));
            sold += held;
        }
        let mk = || {
            fills
                .iter()
                .map(|(i, buy, qty, price)| Fill {
                    id: format!("{round}-{i:02}"),
                    symbol: "X".into(),
                    asset_class: "stock".into(),
                    bucket: "capital",
                    valuation: Valuation::Spot,
                    at: t0 + time::Duration::days(*i as i64 * 3),
                    buy: *buy,
                    qty: *qty,
                    gross: qty * price,
                    fee: 0.0,
                    currency: "EUR".into(),
                    rate: Some(1.0),
                })
                .collect::<Vec<_>>()
        };
        let total = |m| {
            let out = lots::match_fills(mk(), m, false, None);
            assert!(out.errors.is_empty(), "{:?}", out.errors);
            let q: f64 = out.disposals.iter().map(|d| d.qty).sum();
            assert!((q - sold).abs() < 1e-6);
            out.disposals.iter().map(|d| d.gain).sum::<f64>()
        };
        let (a, f, u) = (total(lots::Method::Average), total(lots::Method::Fifo), total(lots::Method::UkPool));
        assert!((a - f).abs() < 1e-6 && (a - u).abs() < 1e-6, "round {round}: {a} {f} {u}");
    }
}

/// Calendars: a year that starts in April or July is bounded correctly.
#[test]
fn calendars() {
    let uk = regime::resolve("uk_cgt", 2025).unwrap().0;
    let (s, e) = uk.year_bounds(2025).unwrap();
    assert_eq!((s.to_string(), e.to_string()), ("2025-04-06".into(), "2026-04-05".into()));
    let au = regime::resolve("au_resident", 2026).unwrap().0;
    let (s, e) = au.year_bounds(2026).unwrap();
    assert_eq!((s.to_string(), e.to_string()), ("2025-07-01".into(), "2026-06-30".into()));
    let fr = regime::resolve("fr_pfu", 2026).unwrap().0;
    let (s, e) = fr.year_bounds(2026).unwrap();
    assert_eq!((s.to_string(), e.to_string()), ("2026-01-01".into(), "2026-12-31".into()));
}
