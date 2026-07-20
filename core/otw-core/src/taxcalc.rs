//! TaxCalculator engine — country rule templates + the pure tax computation.
//!
//! Estimates trading/investing tax ONLY (no personal/income tax). The engine is stateless and
//! pure: given a profile (rates/allowances/rules) and a scenario (inputs), it returns an
//! itemized breakdown showing the math. Country templates are static data here (not a DB table)
//! so they version with the binary. Templates are simplified and may be outdated — the UI shows
//! a disclaimer. Effective rates are best-effort estimates, NOT tax advice.

use serde::Serialize;
use serde_json::{json, Value};

/// A built-in country rule template. `regime` is the stable key stored on profiles.
#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub regime: &'static str,
    pub country: &'static str,
    pub label: &'static str,
    pub person_type: &'static str, // who the defaults describe
    /// Flat rate on gains (percent), if the regime is flat. None => uses income+social.
    pub flat_rate: Option<f64>,
    pub marginal_income_rate: Option<f64>,
    pub social_charges_rate: Option<f64>,
    /// Annual tax-free allowance on capital gains (profile currency units), simplified.
    pub cg_allowance: f64,
    /// Annual tax-free allowance on dividends.
    pub div_allowance: f64,
    /// Long-term relief: gains held >= min_days taxed at `rate` (percent). Empty => none.
    pub holding_relief: &'static [(i64, f64)],
    pub default_currency: &'static str,
    pub source_note: &'static str,
    pub as_of_year: i32,
}

/// The built-in template library. Simplified, for estimation only.
pub fn templates() -> Vec<Template> {
    vec![
        Template {
            regime: "fr_pfu",
            country: "FR",
            label: "France — Individual (PFU / flat tax 30%)",
            person_type: "individual",
            flat_rate: Some(30.0), // 12.8% income + 17.2% social
            marginal_income_rate: Some(12.8),
            social_charges_rate: Some(17.2),
            cg_allowance: 0.0,
            div_allowance: 0.0,
            holding_relief: &[],
            default_currency: "EUR",
            source_note: "FR prélèvement forfaitaire unique (flat 30%).",
            as_of_year: 2026,
        },
        Template {
            regime: "fr_pro",
            country: "FR",
            label: "France — Professional trader (BNC/BIC + social)",
            person_type: "professional",
            flat_rate: None,
            marginal_income_rate: Some(30.0),
            social_charges_rate: Some(17.2),
            cg_allowance: 0.0,
            div_allowance: 0.0,
            holding_relief: &[],
            default_currency: "EUR",
            source_note: "FR pro trading taxed as business income; rate is an example bracket.",
            as_of_year: 2026,
        },
        Template {
            regime: "de_abgeltung",
            country: "DE",
            label: "Germany — Individual (Abgeltungsteuer ~26.375%)",
            person_type: "individual",
            flat_rate: Some(26.375), // 25% + 5.5% Soli
            marginal_income_rate: None,
            social_charges_rate: None,
            cg_allowance: 1000.0, // Sparer-Pauschbetrag (simplified)
            div_allowance: 0.0,   // shares the same Pauschbetrag in reality
            holding_relief: &[],
            default_currency: "EUR",
            source_note: "DE Abgeltungsteuer 25% + Soli; Sparer-Pauschbetrag €1000.",
            as_of_year: 2026,
        },
        Template {
            regime: "uk_cgt",
            country: "GB",
            label: "UK — Individual (CGT + dividend allowance)",
            person_type: "individual",
            flat_rate: Some(20.0), // higher-rate CGT on shares (simplified)
            marginal_income_rate: None,
            social_charges_rate: None,
            cg_allowance: 3000.0,  // annual exempt amount (simplified)
            div_allowance: 500.0,  // dividend allowance
            holding_relief: &[],
            default_currency: "GBP",
            source_note: "UK CGT (simplified 20% higher rate); annual exempt £3000.",
            as_of_year: 2026,
        },
        Template {
            regime: "us_federal",
            country: "US",
            label: "US — Individual (federal LTCG / short-term as income)",
            person_type: "individual",
            // Investing (long-term, >=365d) → 15% bracket; short-term/trading → income rate.
            flat_rate: Some(15.0),
            marginal_income_rate: Some(24.0),
            social_charges_rate: None,
            cg_allowance: 0.0,
            div_allowance: 0.0,
            holding_relief: &[(365, 15.0)],
            default_currency: "USD",
            source_note: "US federal: LTCG 15% bracket; short-term taxed as ordinary income.",
            as_of_year: 2026,
        },
        Template {
            regime: "ch_private",
            country: "CH",
            label: "Switzerland — Individual (private capital gains exempt)",
            person_type: "individual",
            flat_rate: Some(0.0),
            marginal_income_rate: None,
            social_charges_rate: None,
            cg_allowance: 0.0,
            div_allowance: 0.0,
            holding_relief: &[],
            default_currency: "CHF",
            source_note: "CH: private movable-capital gains generally tax-exempt (wealth tax applies).",
            as_of_year: 2026,
        },
        Template {
            regime: "custom_flat",
            country: "",
            label: "Custom — flat rate (fallback for any country)",
            person_type: "individual",
            flat_rate: Some(0.0),
            marginal_income_rate: None,
            social_charges_rate: None,
            cg_allowance: 0.0,
            div_allowance: 0.0,
            holding_relief: &[],
            default_currency: "USD",
            source_note: "Generic fallback: user supplies the rate.",
            as_of_year: 2026,
        },
    ]
}

pub fn template_by_regime(regime: &str) -> Option<Template> {
    templates().into_iter().find(|t| t.regime == regime)
}

/// Effective rate inputs resolved from a profile, falling back to its template.
struct Rates {
    flat_rate: Option<f64>,
    marginal_income_rate: Option<f64>,
    social_charges_rate: Option<f64>,
    cg_allowance: f64,
    div_allowance: f64,
    holding_relief: Vec<(i64, f64)>,
}

/// Resolve the rates the engine will apply: profile overrides win over template defaults.
fn resolve_rates(
    regime: &str,
    flat_override: Option<f64>,
    marginal_override: Option<f64>,
    social_override: Option<f64>,
    allowances: &Value,
    holding_rules: &Value,
) -> Rates {
    let t = template_by_regime(regime);
    let cg_allowance = allowances
        .get("capital_gains")
        .and_then(|v| v.get("annual_free"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| t.as_ref().map(|t| t.cg_allowance).unwrap_or(0.0));
    let div_allowance = allowances
        .get("dividends")
        .and_then(|v| v.get("annual_free"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| t.as_ref().map(|t| t.div_allowance).unwrap_or(0.0));

    // Holding relief from profile if present, else template.
    let holding_relief: Vec<(i64, f64)> = holding_rules
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some((
                        r.get("min_days")?.as_i64()?,
                        r.get("rate")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .filter(|v: &Vec<_>| !v.is_empty())
        .unwrap_or_else(|| {
            t.as_ref()
                .map(|t| t.holding_relief.to_vec())
                .unwrap_or_default()
        });

    Rates {
        flat_rate: flat_override.or_else(|| t.as_ref().and_then(|t| t.flat_rate)),
        marginal_income_rate: marginal_override.or_else(|| t.as_ref().and_then(|t| t.marginal_income_rate)),
        social_charges_rate: social_override.or_else(|| t.as_ref().and_then(|t| t.social_charges_rate)),
        cg_allowance,
        div_allowance,
        holding_relief,
    }
}

/// The base rate (percent) to apply to capital gains for a given context + holding period.
/// Investing long-term gains get holding relief; trading is taxed at the income/flat rate.
fn capital_gains_rate(r: &Rates, context: &str, holding_days: Option<i64>) -> f64 {
    if context == "investing" {
        if let Some(days) = holding_days {
            // Best matching relief tier (highest min_days the holding qualifies for).
            let relief = r
                .holding_relief
                .iter()
                .filter(|(min, _)| days >= *min)
                .max_by_key(|(min, _)| *min)
                .map(|(_, rate)| *rate);
            if let Some(rate) = relief {
                return rate + r.social_charges_rate.unwrap_or(0.0);
            }
        }
        // Relief tiers exist but none matched: the regime distinguishes holding periods,
        // so a short-term (or unknown-holding) gain is taxed as ordinary income when an
        // income rate is defined (e.g. US federal), NOT at the long-term flat rate.
        if !r.holding_relief.is_empty() {
            if let Some(inc) = r.marginal_income_rate {
                return inc + r.social_charges_rate.unwrap_or(0.0);
            }
        }
        // No holding distinction: flat rate if defined, else income+social.
        if let Some(flat) = r.flat_rate {
            return flat;
        }
        return r.marginal_income_rate.unwrap_or(0.0) + r.social_charges_rate.unwrap_or(0.0);
    }
    // Trading: short-term, taxed at income (+social) where defined, else flat.
    match (r.marginal_income_rate, r.flat_rate) {
        (Some(inc), _) => inc + r.social_charges_rate.unwrap_or(0.0),
        (None, Some(flat)) => flat,
        (None, None) => 0.0,
    }
}

/// Compute tax for a scenario. `profile` and `scenario` mirror the store rows (as JSON).
/// Returns the itemized breakdown the UI renders. Pure — no I/O, no FX (caller normalizes).
pub fn compute(profile: &Value, scenario: &Value) -> Value {
    let regime = profile.get("regime").and_then(Value::as_str).unwrap_or("custom_flat");
    let currency = profile.get("currency").and_then(Value::as_str).unwrap_or("USD");
    let flat_override = profile.get("flat_rate").and_then(Value::as_f64);
    let marginal_override = profile.get("marginal_income_rate").and_then(Value::as_f64);
    let social_override = profile.get("social_charges_rate").and_then(Value::as_f64);
    let allowances = profile.get("allowances").cloned().unwrap_or(json!({}));
    let holding_rules = profile.get("holding_period_rules").cloned().unwrap_or(json!([]));

    let rates = resolve_rates(
        regime,
        flat_override,
        marginal_override,
        social_override,
        &allowances,
        &holding_rules,
    );

    let context = scenario.get("context").and_then(Value::as_str).unwrap_or("investing");
    let mode = scenario.get("mode").and_then(Value::as_str).unwrap_or("summary");
    let inputs = scenario.get("inputs").cloned().unwrap_or(json!({}));

    let mut lines: Vec<Value> = Vec::new();
    let mut total_tax = 0.0;
    let mut total_base = 0.0;
    let mut warnings: Vec<String> = Vec::new();

    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);

    if mode == "summary" {
        // Gain = (end - start) - contributions + withdrawals.
        let start = num(&inputs, "start_value");
        let end = num(&inputs, "end_value");
        let contributions = num(&inputs, "contributions");
        let withdrawals = num(&inputs, "withdrawals");
        let gross_gain = (end - start) - contributions + withdrawals;

        // Investing may treat only a realized share as taxable now.
        let realized_pct = inputs
            .get("realized_pct")
            .and_then(Value::as_f64)
            .unwrap_or(if context == "trading" { 100.0 } else { 100.0 });
        let taxable_gain = (gross_gain * realized_pct / 100.0).max(0.0);

        if gross_gain < 0.0 {
            warnings.push("Net loss for the period — no gain to tax (loss may carry forward).".into());
        }
        if context == "investing" && realized_pct < 100.0 {
            warnings.push(format!("Only {realized_pct}% of the gain treated as realized/taxable."));
        }

        let holding_days = inputs.get("holding_days").and_then(Value::as_i64);
        let allowance = rates.cg_allowance;
        let after_allowance = (taxable_gain - allowance).max(0.0);
        let rate = capital_gains_rate(&rates, context, holding_days);
        let tax = after_allowance * rate / 100.0;

        total_base += after_allowance;
        total_tax += tax;
        lines.push(json!({
            "label": "Capital gain",
            "gross": gross_gain,
            "taxable": taxable_gain,
            "allowance": allowance,
            "base": after_allowance,
            "rate_pct": rate,
            "tax": tax,
        }));
    } else {
        // Itemized: one line per income type.
        let item = |label: &str, base: f64, allowance: f64, rate: f64, lines: &mut Vec<Value>, total_tax: &mut f64, total_base: &mut f64| {
            let after = (base - allowance).max(0.0);
            let tax = after * rate / 100.0;
            *total_base += after;
            *total_tax += tax;
            lines.push(json!({
                "label": label, "taxable": base, "allowance": allowance,
                "base": after, "rate_pct": rate, "tax": tax,
            }));
        };

        let holding_days = inputs.get("holding_days").and_then(Value::as_i64);
        let cg_rate = capital_gains_rate(&rates, context, holding_days);

        // Carried losses relieve the three gain buckets (capital → derivative → crypto) in
        // order, so a losing derivative/crypto year isn't taxed on the other buckets. Each
        // bucket only ever floors at 0; the remaining loss cascades to the next bucket.
        let mut remaining_loss = num(&inputs, "prior_losses_carried").abs();
        let mut offset = |raw: f64| -> f64 {
            let applied = raw.min(remaining_loss).max(0.0);
            remaining_loss -= applied;
            (raw - applied).max(0.0)
        };
        let raw_cg = num(&inputs, "realized_capital_gains").max(0.0);
        let raw_deriv = num(&inputs, "derivative_gains").max(0.0);
        let raw_crypto = num(&inputs, "crypto_gains").max(0.0);
        let cg_after_loss = offset(raw_cg);
        let deriv_after_loss = offset(raw_deriv);
        let crypto_after_loss = offset(raw_crypto);

        let prior_losses = num(&inputs, "prior_losses_carried").abs();
        if prior_losses > 0.0 {
            let used = prior_losses - remaining_loss;
            warnings.push(format!(
                "Applied {used} of carried losses across capital/derivative/crypto gains."
            ));
            if remaining_loss > 0.0 {
                warnings.push(format!(
                    "{remaining_loss} of carried losses unused this year (may carry forward)."
                ));
            }
        }

        item("Realized capital gains", cg_after_loss, rates.cg_allowance, cg_rate, &mut lines, &mut total_tax, &mut total_base);
        // Dividends: flat regime rate (or income rate fallback) with dividend allowance.
        let div_rate = rates.flat_rate.unwrap_or_else(|| rates.marginal_income_rate.unwrap_or(0.0));
        item("Dividends", num(&inputs, "dividends"), rates.div_allowance, div_rate, &mut lines, &mut total_tax, &mut total_base);
        item("Interest income", num(&inputs, "interest_income"), 0.0, div_rate, &mut lines, &mut total_tax, &mut total_base);
        // Derivatives/crypto follow the context's capital-gains rate.
        item("Derivative gains", deriv_after_loss, 0.0, cg_rate, &mut lines, &mut total_tax, &mut total_base);
        item("Crypto gains", crypto_after_loss, 0.0, cg_rate, &mut lines, &mut total_tax, &mut total_base);
    }

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
        "disclaimer": "Estimate only — simplified rules, not tax advice. Verify with a professional.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_tax(profile: &Value, scenario: &Value) -> f64 {
        compute(profile, scenario)["total_tax"].as_f64().unwrap()
    }

    /// US federal: a short-term investing gain is taxed as ordinary income (24%), not at
    /// the long-term 15% flat rate; long-term (>=365d) gets the relief tier.
    #[test]
    fn us_federal_short_term_uses_income_rate() {
        let profile = json!({ "regime": "us_federal" });
        let short = json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0, "holding_days": 90 } });
        let long = json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0, "holding_days": 400 } });
        assert_eq!(total_tax(&profile, &short), 240.0);
        assert_eq!(total_tax(&profile, &long), 150.0);
    }

    /// custom_flat: the profile's own flat rate applies to investing gains (previously the
    /// template's 0% always won and the user's rate was ignored).
    #[test]
    fn custom_flat_profile_flat_rate_wins() {
        let profile = json!({ "regime": "custom_flat", "flat_rate": 19.0 });
        let scenario = json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0 } });
        assert_eq!(total_tax(&profile, &scenario), 190.0);
    }

    /// FR PFU stays flat 30% on investing gains (no holding relief defined).
    #[test]
    fn fr_pfu_investing_stays_flat() {
        let profile = json!({ "regime": "fr_pfu" });
        let scenario = json!({ "context": "investing", "mode": "summary",
            "inputs": { "start_value": 0.0, "end_value": 1000.0 } });
        assert_eq!(total_tax(&profile, &scenario), 300.0);
    }

    /// Wealth-tax brackets are sorted before slicing: shuffled input taxes identically.
    #[test]
    fn wealth_tax_brackets_order_independent() {
        let scenario = json!({ "context": "investing", "mode": "summary",
            "inputs": { "portfolio_value_for_wealth_tax": 3_000_000.0 } });
        let sorted = json!({ "regime": "ch_private", "wealth_tax": [
            { "up_to": 1_000_000.0, "rate": 0.1 },
            { "up_to": 2_000_000.0, "rate": 0.2 },
            { "rate": 0.5 }
        ]});
        let shuffled = json!({ "regime": "ch_private", "wealth_tax": [
            { "rate": 0.5 },
            { "up_to": 2_000_000.0, "rate": 0.2 },
            { "up_to": 1_000_000.0, "rate": 0.1 }
        ]});
        // 1M×0.1% + 1M×0.2% + 1M×0.5% = 1000 + 2000 + 5000.
        assert_eq!(total_tax(&sorted, &scenario), 8000.0);
        assert_eq!(total_tax(&shuffled, &scenario), 8000.0);
    }
}
