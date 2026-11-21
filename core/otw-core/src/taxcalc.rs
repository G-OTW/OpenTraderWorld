//! TaxCalculator engine: the regime library and the scenario computation.
//!
//! Estimates investing and trading tax only (no tax on work income). A country's rules are
//! data (`regime`, one TOML file per regime and tax year); the computation is one generic
//! pass (`assess`); which purchase a sale is matched against is `lots`. This file adapts the
//! stored profile and scenario JSON to them. Estimates, not tax advice.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{json, Value};

pub mod assess;
#[cfg(test)]
mod cases;
pub mod lots;
pub mod regime;

use regime::{Regime, CUSTOM};

/// The regime library as the profile editor lists it: every file-backed regime (its latest
/// year) plus the custom one.
pub fn templates() -> Vec<Value> {
    let mut out: Vec<Value> = regime::catalog()
        .into_iter()
        .map(|r| {
            json!({
                "regime": r.id,
                "country": r.country,
                "label": r.label,
                "person_type": r.person_type,
                "default_currency": r.currency,
                "cost_method": r.matching.method,
                "coverage": r.coverage,
                "status": r.status,
                "verified_on": r.verified_on,
                "years": regime::years_of(&r.id),
                "sources": r.sources,
                "source_note": r.notes,
                "inputs": r.inputs,
                "calendar": r.calendar,
                "loss_pools": r.pools,
                "long_after": regime::GAIN_BUCKETS.iter().filter_map(|b| r.long_after(b).map(|d| (b.to_string(), d))).collect::<HashMap<_, _>>(),
                "custom": false,
            })
        })
        .collect();
    out.push(json!({
        "regime": CUSTOM,
        "country": "",
        "label": "Custom rates (any country)",
        "person_type": "individual",
        "default_currency": "USD",
        "cost_method": "fifo",
        "coverage": "simple",
        "status": "draft",
        "years": [],
        "sources": [],
        "source_note": "Your own rates: flat or income + social, optional holding relief.",
        "inputs": ["long_term", "proceeds"],
        "custom": true,
    }));
    out
}

/// The regime a profile computes under in `year`.
pub fn regime_for(profile: &Value, year: i32) -> Regime {
    regime::for_profile(profile, "investing", year).0
}

/// The regime's anti-wash rule, in the lot engine's terms.
pub fn wash_rule(r: &Regime) -> Option<lots::WashRule> {
    r.matching.wash.as_ref().map(|w| lots::WashRule {
        before_days: w.before_days as i64,
        after_days: w.after_days as i64,
        before_months: w.before_months,
        after_months: w.after_months,
        classes: w.classes.clone(),
    })
}

/// A netting pool with its carry-forward rule.
#[derive(Debug, Clone, Serialize)]
pub struct PoolRule {
    pub key: String,
    pub buckets: Vec<String>,
    pub years: Option<u32>,
    pub spill_to: Vec<String>,
}

/// The profile's netting pools in `year`.
pub fn pool_rules(profile: &Value, year: i32) -> Vec<PoolRule> {
    regime_for(profile, year)
        .pools
        .into_iter()
        .map(|p| PoolRule { key: p.key, buckets: p.buckets, years: p.carry_years, spill_to: p.spill_to })
        .collect()
}

/// The profile's cost method: its own override, else its regime's.
pub fn cost_method(profile: &Value, year: i32) -> lots::Method {
    profile
        .get("cost_method")
        .and_then(Value::as_str)
        .and_then(lots::Method::parse)
        .unwrap_or_else(|| regime_for(profile, year).method())
}

/// One loss still available in a later year.
#[derive(Debug, Clone, Serialize)]
pub struct CarriedLoss {
    pub origin: i32,
    pub left: f64,
    /// Last tax year it can be used in. None = no limit.
    pub expires: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PoolCarry {
    pub pool: String,
    pub years: Option<u32>,
    pub available: f64,
    pub losses: Vec<CarriedLoss>,
}

/// What each pool still carries into `year`, from the registry's per-year nets (`rows`:
/// year, pool, net). Year by year: a pool's gain consumes its own losses, oldest first;
/// then a pool that spills (DE general pot) offsets the gain left in its target pools, this
/// year's loss first, then the oldest carried. A loss past its last year drops out. Derived
/// on read: fixing an old year fixes every later one.
pub fn carry_into(rows: &[(i32, String, f64)], rules: &[PoolRule], year: i32) -> Vec<PoolCarry> {
    use std::collections::{BTreeSet, VecDeque};
    let mut losses: HashMap<String, VecDeque<(i32, f64)>> =
        rules.iter().map(|r| (r.key.clone(), VecDeque::new())).collect();
    let consume = |q: &mut VecDeque<(i32, f64)>, gain: &mut f64, newest_first: bool| {
        while *gain > 0.0 {
            let slot = if newest_first { q.back_mut() } else { q.front_mut() };
            let Some(l) = slot else { break };
            let used = gain.min(l.1);
            l.1 -= used;
            *gain -= used;
            if l.1 <= 1e-9 {
                if newest_first {
                    q.pop_back();
                } else {
                    q.pop_front();
                }
            }
        }
    };
    let years: BTreeSet<i32> = rows.iter().map(|(y, _, _)| *y).filter(|y| *y < year).collect();
    for y in years {
        let mut gain_left: HashMap<String, f64> = HashMap::new();
        let mut fresh: HashMap<String, bool> = HashMap::new();
        for r in rules {
            let q = losses.get_mut(&r.key).unwrap();
            if let Some(n) = r.years {
                q.retain(|(o, _)| y - o <= n as i32);
            }
            let net: f64 = rows.iter().filter(|(ry, p, _)| *ry == y && *p == r.key).map(|(_, _, n)| n).sum();
            if net < 0.0 {
                if r.years != Some(0) {
                    q.push_back((y, -net));
                    fresh.insert(r.key.clone(), true);
                }
                continue;
            }
            let mut gain = net;
            consume(q, &mut gain, false);
            gain_left.insert(r.key.clone(), gain);
        }
        for r in rules.iter().filter(|r| !r.spill_to.is_empty()) {
            for t in &r.spill_to {
                let mut gain = gain_left.get(t).copied().unwrap_or(0.0);
                let q = losses.get_mut(&r.key).unwrap();
                if fresh.get(&r.key).copied().unwrap_or(false) {
                    consume(q, &mut gain, true);
                }
                consume(q, &mut gain, false);
                gain_left.insert(t.clone(), gain);
            }
        }
    }
    rules
        .iter()
        .map(|r| {
            let mut q = losses.remove(&r.key).unwrap_or_default();
            if let Some(n) = r.years {
                q.retain(|(o, _)| year - o <= n as i32);
            }
            let losses: Vec<CarriedLoss> = q
                .into_iter()
                .map(|(origin, left)| CarriedLoss { origin, left, expires: r.years.map(|n| origin + n as i32) })
                .collect();
            PoolCarry {
                pool: r.key.clone(),
                years: r.years,
                available: losses.iter().map(|l| l.left).sum(),
                losses,
            }
        })
        .collect()
}

/// Compute tax for a scenario. `profile` and `scenario` mirror the store rows (as JSON).
/// Returns the itemized breakdown the UI renders. Pure: no I/O, no FX (caller normalizes).
pub fn compute(profile: &Value, scenario: &Value) -> Value {
    let currency = profile.get("currency").and_then(Value::as_str).unwrap_or("USD");
    let context = scenario.get("context").and_then(Value::as_str).unwrap_or("investing");
    let mode = scenario.get("mode").and_then(Value::as_str).unwrap_or("summary");
    let inputs = scenario.get("inputs").cloned().unwrap_or(json!({}));
    let year = scenario
        .get("tax_year")
        .and_then(Value::as_i64)
        .map(|y| y as i32)
        .unwrap_or_else(|| time::OffsetDateTime::now_utc().year());

    let (reg, year_warning) = regime::for_profile(profile, context, year);
    let input = assess::input_from_form(&reg, mode, &inputs, profile);
    let a = assess::assess(&reg, &input);

    let mut warnings: Vec<String> = year_warning.into_iter().collect();
    warnings.extend(a.warnings);
    let mut lines: Vec<Value> = a.lines.iter().map(|l| json!(l)).collect();
    lines.extend(a.surcharges);
    let total_base = a.total_base;
    let mut total_tax = a.total_tax;

    // Wealth tax on a portfolio snapshot, if the profile defines brackets.
    if let Some(brackets) = profile.get("wealth_tax").and_then(Value::as_array) {
        let pv = inputs
            .get("portfolio_value_for_wealth_tax")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if pv > 0.0 && !brackets.is_empty() {
            // Marginal brackets: [{ up_to, rate }], up_to absent = top bracket. Sort by cap
            // before slicing — an unsorted profile would otherwise produce negative slices
            // and tax later brackets against the wrong base.
            let mut rows: Vec<(f64, f64)> = brackets
                .iter()
                .map(|b| {
                    (
                        b.get("up_to").and_then(Value::as_f64).unwrap_or(f64::INFINITY),
                        b.get("rate").and_then(Value::as_f64).unwrap_or(0.0),
                    )
                })
                .collect();
            rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut remaining = pv;
            let mut prev_cap = 0.0;
            let mut wealth_tax = 0.0;
            for (cap, rate) in rows {
                let slice = (cap - prev_cap).min(remaining).max(0.0);
                wealth_tax += slice * rate / 100.0;
                remaining -= slice;
                prev_cap = cap;
                if remaining <= 0.0 {
                    break;
                }
            }
            total_tax += wealth_tax;
            lines.push(json!({
                "label": "Wealth tax",
                "taxable": pv, "base": pv, "rate_pct": Value::Null, "tax": wealth_tax,
            }));
        }
    }

    let effective_rate = if total_base > 0.0 { total_tax / total_base * 100.0 } else { 0.0 };

    json!({
        "currency": currency,
        "context": context,
        "mode": mode,
        "lines": lines,
        "total_base": total_base,
        "total_tax": total_tax,
        "effective_rate_pct": effective_rate,
        "warnings": warnings,
        "pools": a.pools,
        "regime": {
            "id": reg.id,
            "year": reg.year,
            "label": reg.label,
            "coverage": reg.coverage,
            "status": reg.status,
            "verified_on": reg.verified_on,
            "sources": reg.sources,
            "notes": reg.notes,
        },
        "disclaimer": "Estimate only: simplified rules, not tax advice. Verify with a professional.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_tax(profile: &Value, scenario: &Value) -> f64 {
        compute(profile, scenario)["total_tax"].as_f64().unwrap()
    }

    /// custom_flat: the profile's own flat rate applies to investing gains.
    #[test]
    fn custom_flat_profile_flat_rate_wins() {
        let profile = json!({ "regime": "custom_flat", "flat_rate": 19.0 });
        let scenario = json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0 } });
        assert_eq!(total_tax(&profile, &scenario), 190.0);
    }

    /// custom_flat with a relief tier: long holdings take the tier rate, short ones the
    /// income rate.
    #[test]
    fn custom_holding_tiers() {
        let profile = json!({ "regime": "custom_flat", "flat_rate": 15.0, "marginal_income_rate": 24.0,
            "holding_period_rules": [{ "min_days": 365, "rate": 15.0 }] });
        let s = |d| json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0, "holding_days": d } });
        assert_eq!(total_tax(&profile, &s(90)), 240.0);
        assert_eq!(total_tax(&profile, &s(400)), 150.0);
    }

    /// Registry carry: oldest first, consumed by later gains, expired after the regime's years.
    #[test]
    fn carry_is_derived_and_expires() {
        let rules = pool_rules(&json!({ "regime": "fr_pfu" }), 2025);
        let rows = vec![
            (2012, "securities".to_string(), -1000.0),
            (2014, "securities".to_string(), -500.0),
            (2015, "securities".to_string(), 300.0),
        ];
        let c = carry_into(&rows, &rules, 2020);
        let sec = c.iter().find(|p| p.pool == "securities").unwrap();
        assert!((sec.available - 1200.0).abs() < 1e-9);
        assert_eq!(sec.losses[0].origin, 2012);
        assert_eq!(sec.losses[0].expires, Some(2022));
        let c = carry_into(&rows, &rules, 2023);
        let sec = c.iter().find(|p| p.pool == "securities").unwrap();
        assert!((sec.available - 500.0).abs() < 1e-9);
    }

    /// DE: a general-pot loss carried in the registry is used against a later share gain;
    /// a share loss is never used against the general pot.
    #[test]
    fn carry_spills_one_way() {
        let rules = pool_rules(&json!({ "regime": "de_abgeltung" }), 2026);
        let rows = vec![
            (2024, "general".to_string(), -1000.0),
            (2024, "shares".to_string(), -700.0),
            (2025, "shares".to_string(), 400.0),
            (2025, "general".to_string(), 300.0),
        ];
        let c = carry_into(&rows, &rules, 2026);
        let get = |k: &str| c.iter().find(|p| p.pool == k).unwrap().available;
        // shares: 700 - 400 = 300 left. general: 1000 - 300 own gain = 700, nothing left
        // in shares to spill into (its gain went to its own loss).
        assert!((get("shares") - 300.0).abs() < 1e-9);
        assert!((get("general") - 700.0).abs() < 1e-9);
        let rows = vec![(2024, "general".to_string(), -1000.0), (2025, "shares".to_string(), 600.0)];
        let c = carry_into(&rows, &rules, 2026);
        assert!((c.iter().find(|p| p.pool == "general").unwrap().available - 400.0).abs() < 1e-9);
    }

    /// "More than one year": a sale on the anniversary is short, the day after is long,
    /// leap years included.
    #[test]
    fn anniversary_holding() {
        let r = regime::resolve("us_federal", 2026).unwrap().0;
        let d = |s: &str| regime::parse_ymd(s).unwrap();
        assert_eq!(r.term_between("capital", d("2024-02-01"), d("2025-02-01")), "short");
        assert_eq!(r.term_between("capital", d("2024-02-01"), d("2025-02-02")), "long");
        assert_eq!(r.term_between("capital", d("2024-02-29"), d("2025-03-01")), "short");
        assert_eq!(r.term_between("capital", d("2024-02-29"), d("2025-03-02")), "long");
    }

    /// Wealth-tax brackets are sorted before slicing: shuffled input taxes identically.
    #[test]
    fn wealth_tax_brackets_order_independent() {
        let scenario = json!({ "context": "investing", "mode": "summary",
            "inputs": { "portfolio_value_for_wealth_tax": 3_000_000.0 } });
        let sorted = json!({ "regime": "custom_flat", "wealth_tax": [
            { "up_to": 1_000_000.0, "rate": 0.1 },
            { "up_to": 2_000_000.0, "rate": 0.2 },
            { "rate": 0.5 }
        ]});
        let shuffled = json!({ "regime": "custom_flat", "wealth_tax": [
            { "rate": 0.5 },
            { "up_to": 2_000_000.0, "rate": 0.2 },
            { "up_to": 1_000_000.0, "rate": 0.1 }
        ]});
        assert_eq!(total_tax(&sorted, &scenario), 8000.0);
        assert_eq!(total_tax(&shuffled, &scenario), 8000.0);
    }
}
