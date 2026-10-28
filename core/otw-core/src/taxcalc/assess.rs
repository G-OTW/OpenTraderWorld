//! The generic assessment: one pass of fixed steps, every one parameterized by the regime.
//!
//!   1. exempt items drop out (DE crypto after a year, CH private gains);
//!   2. gains and losses net inside their pool, then the pool's carried loss applies;
//!   3. allowances: subtracted (Freibetrag) or all-or-nothing (Freigrenze, FR 305 EUR);
//!   4. inclusion shares (AU discount, CA inclusion rate);
//!   5. schedules: marginal brackets, optionally stacked on other income, or the
//!      taxpayer's own marginal rate;
//!   6. surcharges on the base (social charges, NIIT) or on the tax (Soli).
//!
//! Pure: no I/O. A country never adds code here, only parameters to its file; a rule no
//! step covers becomes a new generic step, tested once, available to every country.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::{json, Value};

use super::regime::{
    AllowanceKind, Item, Measure, Regime, ScheduleKind, SurchargeOn, GAIN_BUCKETS,
};

/// What the assessment needs, already split into items.
#[derive(Debug, Clone, Default)]
pub struct Input {
    /// Signed amounts per item (gains may be negative, income never is).
    pub amounts: BTreeMap<Item, f64>,
    /// Sale proceeds per bucket, for allowances measured on proceeds.
    pub proceeds: HashMap<String, f64>,
    pub other_income: f64,
    pub marginal_rate: Option<f64>,
    /// Carried losses per pool from the registry. None = use `legacy_prior_losses`.
    pub carried: Option<HashMap<String, f64>>,
    pub legacy_prior_losses: f64,
    /// Label of the capital line in summary mode.
    pub summary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub label: String,
    pub item: String,
    pub schedule: String,
    /// After netting and carried losses, before the allowance.
    pub taxable: f64,
    pub allowance: f64,
    /// What the schedule taxes (after allowance and inclusion share).
    pub base: f64,
    pub rate_pct: Option<f64>,
    pub tax: f64,
    pub exempt: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Assessment {
    pub lines: Vec<Line>,
    pub surcharges: Vec<Value>,
    pub pools: Vec<Value>,
    pub warnings: Vec<String>,
    pub total_base: f64,
    pub total_tax: f64,
}

/// Build the item split of the form's inputs under `regime`. `long_term.<bucket>` is the
/// long-term share of that bucket (first holding tier); `holding_days` classifies a whole
/// bucket at once (the older single field).
pub fn input_from_form(regime: &Regime, mode: &str, inputs: &Value, profile: &Value) -> Input {
    let num = |k: &str| inputs.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let mut raw: Vec<(&str, f64)> = Vec::new();
    let summary = mode == "summary";
    if summary {
        let gain = (num("end_value") - num("start_value")) - num("contributions") + num("withdrawals");
        let pct = inputs.get("realized_pct").and_then(Value::as_f64).unwrap_or(100.0);
        raw.push(("capital", gain * pct / 100.0));
    } else {
        raw.push(("capital", num("realized_capital_gains")));
        raw.push(("derivative", num("derivative_gains")));
        raw.push(("crypto", num("crypto_gains")));
        raw.push(("dividends", num("dividends").max(0.0)));
        raw.push(("interest", num("interest_income").max(0.0)));
    }
    let holding_days = inputs.get("holding_days").and_then(Value::as_i64);
    let long = inputs.get("long_term");
    let mut amounts = BTreeMap::new();
    for (b, v) in raw {
        if !GAIN_BUCKETS.contains(&b) {
            amounts.insert(Item { bucket: b.into(), term: None }, v);
            continue;
        }
        let long_part = long.and_then(|l| l.get(b)).and_then(Value::as_f64);
        match (long_part, regime.long_after(b)) {
            (Some(l), Some(days)) => {
                let term = regime.term_for(b, days);
                *amounts.entry(Item { bucket: b.into(), term: Some(term) }).or_insert(0.0) += l;
                *amounts.entry(Item { bucket: b.into(), term: Some("short".into()) }).or_insert(0.0) += v - l;
            }
            _ => {
                let term = holding_days.map(|d| regime.term_for(b, d)).unwrap_or_else(|| "short".into());
                *amounts.entry(Item { bucket: b.into(), term: Some(term) }).or_insert(0.0) += v;
            }
        }
    }
    let proceeds = inputs
        .get("proceeds")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect())
        .unwrap_or_default();
    let carried = inputs
        .get("carried_losses")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?.abs()))).collect());
    Input {
        amounts,
        proceeds,
        other_income: num("other_income").max(0.0),
        marginal_rate: inputs
            .get("marginal_rate")
            .and_then(Value::as_f64)
            .or_else(|| profile.get("marginal_income_rate").and_then(Value::as_f64)),
        carried,
        legacy_prior_losses: num("prior_losses_carried").abs(),
        summary,
    }
}

/// Tax on `x` under marginal brackets.
fn bracket_tax(brackets: &[super::regime::Bracket], x: f64) -> f64 {
    let mut tax = 0.0;
    let mut prev = 0.0;
    for b in brackets {
        let cap = b.up_to.unwrap_or(f64::INFINITY);
        if x > prev {
            tax += (x.min(cap) - prev) * b.rate / 100.0;
        }
        prev = cap;
    }
    tax
}

/// Short-term items first: a loss is absorbed by the most heavily taxed gain first, which
/// is what netting short against short before long against long comes to.
fn term_rank(it: &Item) -> u8 {
    match it.term.as_deref() {
        Some("short") | None => 0,
        _ => 1,
    }
}

pub fn assess(regime: &Regime, input: &Input) -> Assessment {
    let mut out = Assessment::default();
    let items = regime.items();
    let mut amount: BTreeMap<Item, f64> = items
        .iter()
        .map(|it| (it.clone(), input.amounts.get(it).copied().unwrap_or(0.0)))
        .collect();
    for (it, v) in &input.amounts {
        if !amount.contains_key(it) && *v != 0.0 {
            out.warnings.push(format!("{} is not an item of this regime and was left out.", it.name()));
        }
    }

    // 1. Exemptions.
    let is_exempt = |it: &Item| regime.exempt.iter().any(|e| e.items.iter().any(|s| it.matches(s)));
    let exempt_gross: BTreeMap<Item, f64> =
        amount.iter().filter(|(it, _)| is_exempt(it)).map(|(k, v)| (k.clone(), *v)).collect();
    for it in exempt_gross.keys() {
        amount.insert(it.clone(), 0.0);
    }

    // 2. Pools: net inside, then carried losses.
    let allowance_for = |bucket: &str| -> f64 {
        regime
            .allowances
            .iter()
            .filter(|a| a.kind == AllowanceKind::Allowance && a.items.iter().any(|s| s.split('.').next() == Some(bucket)))
            .map(|a| a.amount)
            .next()
            .unwrap_or(0.0)
    };
    let mut taxable: BTreeMap<Item, f64> = amount.iter().map(|(k, v)| (k.clone(), v.max(0.0))).collect();
    // What each pool has left after its own gains: this year's loss and the carried loss.
    let mut leftover: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let mut members_of: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for p in &regime.pools {
        let mut members: Vec<Item> = amount
            .keys()
            .filter(|it| p.buckets.iter().any(|b| *b == it.bucket) && !is_exempt(it))
            .cloned()
            .collect();
        if members.is_empty() {
            continue;
        }
        members.sort_by_key(term_rank);
        let net: f64 = members.iter().map(|it| amount[it]).sum();
        let mut absorb: f64 = members.iter().map(|it| (-amount[it]).max(0.0)).sum();
        for it in &members {
            let t = taxable.get_mut(it).unwrap();
            let a = t.min(absorb);
            *t -= a;
            absorb -= a;
        }
        let carried_in = input.carried.as_ref().and_then(|m| m.get(&p.key)).copied().unwrap_or(0.0);
        let positive: f64 = members.iter().map(|it| taxable[it]).sum();
        let usable = if p.preserve_allowance {
            let floor = p.buckets.iter().map(|b| allowance_for(b)).fold(0.0_f64, f64::max);
            (positive - floor).max(0.0)
        } else {
            positive
        };
        let used = carried_in.min(usable);
        let mut left = used;
        for it in &members {
            let t = taxable.get_mut(it).unwrap();
            let c = t.min(left);
            *t -= c;
            left -= c;
        }
        leftover.insert(p.key.clone(), (absorb, carried_in - used));
        members_of.insert(p.key.clone(), members);
        out.pools.push(json!({
            "pool": p.key,
            "net": net,
            "carried_in": carried_in,
            "carried_used": used,
            "carried_left": carried_in - used,
            "loss_created": if net < 0.0 && p.carry_years != Some(0) { -net } else { 0.0 },
            "years": p.carry_years,
        }));
    }
    // One-way spills: a pool's leftover loss, this year's first, then carried, offsets the
    // gains left in the pools it may reach.
    for p in regime.pools.iter().filter(|p| !p.spill_to.is_empty()) {
        let (mut year_loss, mut carry) = leftover.get(&p.key).copied().unwrap_or((0.0, 0.0));
        let (y0, c0) = (year_loss, carry);
        for t in &p.spill_to {
            for it in members_of.get(t).cloned().unwrap_or_default() {
                let v = taxable.get_mut(&it).unwrap();
                let a = v.min(year_loss);
                *v -= a;
                year_loss -= a;
                let c = v.min(carry);
                *v -= c;
                carry -= c;
            }
        }
        if let Some(r) = out.pools.iter_mut().find(|r| r["pool"] == p.key.as_str()) {
            let lc = r["loss_created"].as_f64().unwrap_or(0.0);
            if lc > 0.0 {
                r["loss_created"] = json!(year_loss.min(lc));
            }
            r["carried_used"] = json!(r["carried_used"].as_f64().unwrap_or(0.0) + (c0 - carry));
            r["carried_left"] = json!(carry);
            r["spilled"] = json!((y0 - year_loss) + (c0 - carry));
        }
    }
    // The older hand-typed figure, when the registry did not answer.
    if input.carried.is_none() && input.legacy_prior_losses > 0.0 {
        let mut left = input.legacy_prior_losses;
        let mut order: Vec<Item> = taxable.keys().filter(|it| GAIN_BUCKETS.contains(&it.bucket.as_str())).cloned().collect();
        order.sort_by_key(|it| (GAIN_BUCKETS.iter().position(|b| *b == it.bucket), term_rank(it)));
        for it in order {
            let t = taxable.get_mut(&it).unwrap();
            let c = t.min(left);
            *t -= c;
            left -= c;
        }
        let used = input.legacy_prior_losses - left;
        out.warnings.push(format!("Applied {used} of carried losses across capital/derivative/crypto gains."));
        if left > 0.0 {
            out.warnings.push(format!("{left} of carried losses unused this year (may carry forward)."));
        }
    }

    // 3. Allowances.
    let mut allowance: BTreeMap<Item, f64> = BTreeMap::new();
    for a in &regime.allowances {
        let mut members: Vec<Item> = taxable
            .keys()
            .filter(|it| a.items.iter().any(|s| it.matches(s)) && !is_exempt(it))
            .cloned()
            .collect();
        members.sort_by_key(term_rank);
        let total: f64 = members.iter().map(|it| taxable[it]).sum();
        if total <= 0.0 {
            continue;
        }
        match a.kind {
            AllowanceKind::Allowance => {
                let mut left = a.amount;
                for it in &members {
                    let t = taxable.get_mut(it).unwrap();
                    let c = t.min(left);
                    *t -= c;
                    left -= c;
                    *allowance.entry(it.clone()).or_insert(0.0) += c;
                }
            }
            AllowanceKind::Cliff => {
                let measured = match a.measure {
                    Measure::Gain => Some(total),
                    Measure::Proceeds => {
                        let mut buckets: Vec<&str> = members.iter().map(|it| it.bucket.as_str()).collect();
                        buckets.dedup();
                        let known: Vec<f64> = buckets.iter().filter_map(|b| input.proceeds.get(*b).copied()).collect();
                        (!known.is_empty()).then(|| known.iter().sum())
                    }
                };
                match measured {
                    Some(m) if (if a.strict { m < a.amount } else { m <= a.amount }) => {
                        for it in &members {
                            let t = taxable.get_mut(it).unwrap();
                            *allowance.entry(it.clone()).or_insert(0.0) += *t;
                            *t = 0.0;
                        }
                        out.warnings.push(format!(
                            "{}: at or under {} ({m:.2}), nothing due.",
                            members.iter().map(Item::name).collect::<Vec<_>>().join(", "),
                            a.amount
                        ));
                    }
                    Some(_) => {}
                    None => out.warnings.push(format!(
                        "The {} threshold on {} is measured on sale proceeds, which were not given: not applied.",
                        a.amount,
                        a.items.join(", ")
                    )),
                }
            }
        }
    }

    // 4. Inclusion shares.
    let mut base: BTreeMap<Item, f64> = taxable.clone();
    for inc in &regime.inclusion {
        for (it, b) in base.iter_mut() {
            if inc.items.iter().any(|s| it.matches(s)) {
                *b *= inc.share;
            }
        }
    }

    // 5. Schedules.
    let mut tax: BTreeMap<Item, f64> = BTreeMap::new();
    let mut schedule_of: BTreeMap<Item, String> = BTreeMap::new();
    let mut scheds: Vec<&super::regime::Schedule> = regime.schedules.iter().collect();
    scheds.sort_by_key(|s| s.order);
    let mut running = input.other_income;
    let mut marginal_missing = false;
    for s in scheds {
        let members: Vec<Item> =
            base.keys().filter(|it| s.items.iter().any(|sel| it.matches(sel)) && !is_exempt(it)).cloned().collect();
        let b: f64 = members.iter().map(|it| base[it]).sum();
        for it in &members {
            schedule_of.insert(it.clone(), s.label.clone());
        }
        let t = match s.kind {
            ScheduleKind::Brackets if s.stack => {
                let t = bracket_tax(&s.brackets, running + b) - bracket_tax(&s.brackets, running);
                running += b;
                t
            }
            ScheduleKind::Brackets => bracket_tax(&s.brackets, b),
            ScheduleKind::Marginal => match input.marginal_rate {
                Some(r) => b * r / 100.0,
                None => {
                    marginal_missing |= b > 0.0;
                    0.0
                }
            },
        };
        for it in &members {
            let share = if b > 0.0 { base[it] / b } else { 0.0 };
            tax.insert(it.clone(), t * share);
        }
    }
    if marginal_missing {
        out.warnings.push("Enter your marginal income-tax rate: part of this regime taxes at it.".into());
    }

    // Lines.
    for it in &items {
        let exempt = is_exempt(it);
        let gross = if exempt { exempt_gross.get(it).copied().unwrap_or(0.0) } else { amount[it] };
        let (tx, bs, al) = (tax.get(it).copied().unwrap_or(0.0), base[it], allowance.get(it).copied().unwrap_or(0.0));
        if gross == 0.0 && tx == 0.0 && bs == 0.0 && al == 0.0 {
            continue;
        }
        out.lines.push(Line {
            label: label(it, input.summary),
            item: it.name(),
            schedule: schedule_of.get(it).cloned().unwrap_or_else(|| "exempt".into()),
            taxable: if exempt { gross } else { taxable[it] + al },
            allowance: al,
            base: bs,
            rate_pct: (bs > 0.0).then(|| tx / bs * 100.0),
            tax: tx,
            exempt,
        });
        out.total_base += bs;
        out.total_tax += tx;
    }

    // 6. Surcharges.
    for sc in &regime.surcharges {
        let members: Vec<&Item> = base.keys().filter(|it| sc.items.iter().any(|s| it.matches(s)) && !is_exempt(it)).collect();
        let b: f64 = members.iter().map(|it| base[*it]).sum();
        let (on, t) = match sc.on {
            SurchargeOn::Base => {
                let over = (input.other_income + b - sc.threshold).max(0.0).min(b);
                (over, over * sc.rate / 100.0)
            }
            SurchargeOn::Tax => {
                let on: f64 = members.iter().map(|it| tax.get(*it).copied().unwrap_or(0.0)).sum();
                (on, on * sc.rate / 100.0)
            }
        };
        if t == 0.0 {
            continue;
        }
        out.total_tax += t;
        out.surcharges.push(json!({
            "label": sc.label,
            "taxable": on,
            "base": on,
            "rate_pct": sc.rate,
            "tax": t,
        }));
    }
    out
}

fn label(it: &Item, summary: bool) -> String {
    let b = match it.bucket.as_str() {
        "capital" if summary => "Capital gain",
        "capital" => "Realized capital gains",
        "derivative" => "Derivative gains",
        "crypto" => "Crypto gains",
        "dividends" => "Dividends",
        "interest" => "Interest income",
        other => other,
    };
    match it.term.as_deref() {
        None | Some("short") => b.to_string(),
        Some(t) => format!("{b} ({t})"),
    }
}
