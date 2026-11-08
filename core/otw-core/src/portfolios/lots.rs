//! Tax lots and loss-harvesting candidates, under the Tax Calculator's rules.
//!
//! The method that decides which lot a sale consumes, the holding threshold that makes a
//! lot long-term, and the anti-wash window all come from the Tax Calculator regime of the
//! first profile (by name). No profile means FIFO, no holding tiers and no wash rule, and
//! the answer says so: a rule nobody configured is not a rule to apply silently.

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::{Date, Duration};
use uuid::Uuid;

use otw_store::portfolios::ledger::{self, Ledger, LedgerOp, Lot, Method};
use otw_store::portfolios::{self as store, Asset, Portfolio};

use crate::taxcalc;

/// The anti-wash window around a sale, as the regime states it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WashWindow {
    pub before_days: i64,
    pub after_days: i64,
    pub before_months: u32,
    pub after_months: u32,
    /// Asset classes covered. Empty = all.
    pub classes: Vec<String>,
}

impl WashWindow {
    pub fn covers(&self, class: &str) -> bool {
        self.classes.is_empty() || self.classes.iter().any(|c| c == class)
    }
    pub fn start(&self, day: Date) -> Date {
        add_months(day, -(self.before_months as i32)) - Duration::days(self.before_days)
    }
    pub fn end(&self, day: Date) -> Date {
        add_months(day, self.after_months as i32) + Duration::days(self.after_days)
    }
}

fn add_months(d: Date, months: i32) -> Date {
    if months == 0 {
        return d;
    }
    let total = d.year() * 12 + (d.month() as i32 - 1) + months;
    let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u8);
    let month = time::Month::try_from(m).expect("1..=12");
    let day = d.day().min(time::util::days_in_month(month, y));
    Date::from_calendar_date(y, month, day).unwrap_or(d)
}

/// The rules the lots are read under, and where they came from.
pub struct TaxRules {
    pub method: Method,
    /// `(id, name)` of the profile the rules were read from. None = defaults.
    pub profile: Option<(Uuid, String)>,
    pub regime: Option<taxcalc::regime::Regime>,
    pub wash: Option<WashWindow>,
}

impl TaxRules {
    pub fn defaults() -> Self {
        Self { method: Method::Fifo, profile: None, regime: None, wash: None }
    }

    /// The regime's holding bucket for an asset class.
    fn bucket(class: &str) -> &'static str {
        if class == "crypto" { "crypto" } else { "capital" }
    }

    /// The term a lot opened on `opened` has reached on `today`, when a regime says.
    pub fn term(&self, class: &str, opened: Date, today: Date) -> Option<String> {
        self.regime.as_ref().map(|r| r.term_between(Self::bucket(class), opened, today))
    }

    pub fn long_after(&self, class: &str) -> Option<i64> {
        self.regime.as_ref().and_then(|r| r.long_after(Self::bucket(class)))
    }

    pub fn describe(&self) -> Value {
        json!({
            "method": self.method.key(),
            "profile_id": self.profile.as_ref().map(|p| p.0),
            "profile_name": self.profile.as_ref().map(|p| p.1.clone()),
            "regime": self.regime.as_ref().map(|r| r.id.clone()),
            "wash": self.wash,
            // Said out loud: FIFO and no wash rule are what nothing configured means.
            "defaulted": self.profile.is_none(),
        })
    }
}

/// Read the rules from the first Tax Calculator profile, else the defaults.
pub async fn tax_rules(pool: &PgPool, today: Date) -> TaxRules {
    let profiles = match otw_store::taxcalc::list_profiles(pool).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("portfolio lots: reading tax profiles: {e:#}");
            return TaxRules::defaults();
        }
    };
    let Some(p) = profiles.into_iter().next() else {
        return TaxRules::defaults();
    };
    let Ok(v) = serde_json::to_value(&p) else {
        return TaxRules::defaults();
    };
    let year = today.year();
    let method = Method::parse(taxcalc::cost_method(&v, year).key()).unwrap_or(Method::Fifo);
    let regime = taxcalc::regime_for(&v, year);
    let wash = regime.matching.wash.as_ref().map(|w| WashWindow {
        before_days: w.before_days as i64,
        after_days: w.after_days as i64,
        before_months: w.before_months,
        after_months: w.after_months,
        classes: w.classes.clone(),
    });
    TaxRules { method, profile: Some((p.id, p.name)), regime: Some(regime), wash }
}

/// One open lot as the screen shows it.
#[derive(Debug, Clone, Serialize)]
pub struct LotView {
    pub asset_id: Uuid,
    pub symbol: String,
    pub opened: String,
    pub qty: f64,
    pub cost: f64,
    pub cost_per_unit: f64,
    pub value: Option<f64>,
    pub unrealized: Option<f64>,
    pub holding_days: i64,
    /// The regime's term (`short`, `long`, ...), None without a regime.
    pub term: Option<String>,
}

pub fn lot_views(asset: &Asset, lots: &[Lot], price: Option<f64>, rules: &TaxRules, today: Date) -> Vec<LotView> {
    lots.iter()
        .map(|l| {
            let value = price.map(|p| p * l.qty);
            LotView {
                asset_id: asset.id,
                symbol: asset.symbol.clone(),
                opened: l.opened.to_string(),
                qty: l.qty,
                cost: l.cost,
                cost_per_unit: if l.qty > 0.0 { l.cost / l.qty } else { 0.0 },
                value,
                unrealized: value.map(|v| v - l.cost),
                holding_days: ledger::holding_days(l.opened, today),
                term: rules.term(&asset.asset_class, l.opened, today),
            }
        })
        .collect()
}

/// A lot under water, with whether selling it today would run into the wash rule.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    #[serde(flatten)]
    pub lot: LotView,
    /// A buy of the same asset inside the window before today: selling now would defer the
    /// loss onto those shares.
    pub wash_flag: bool,
    pub last_buy: Option<String>,
}

/// Every losing lot, worst first. `buys` are `(asset, day)` of every purchase.
pub fn harvest(
    lots: Vec<(LotView, String)>,
    buys: &[(Uuid, Date)],
    wash: Option<&WashWindow>,
    today: Date,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = lots
        .into_iter()
        .filter(|(l, _)| l.unrealized.is_some_and(|u| u < 0.0))
        .map(|(lot, class)| {
            let last_buy = buys.iter().filter(|(a, _)| *a == lot.asset_id).map(|(_, d)| *d).max();
            let wash_flag = match wash {
                Some(w) if w.covers(&class) => {
                    let start = w.start(today);
                    buys.iter().any(|(a, d)| *a == lot.asset_id && *d >= start && *d <= today)
                }
                _ => false,
            };
            Candidate { lot, wash_flag, last_buy: last_buy.map(|d| d.to_string()) }
        })
        .collect();
    out.sort_by(|a, b| {
        a.lot.unrealized.unwrap_or(0.0).partial_cmp(&b.lot.unrealized.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The whole portfolio walked under `method`, plus the raw rows (the buys list reads them).
pub async fn walk(pool: &PgPool, pf: &Portfolio, method: Method) -> anyhow::Result<(Ledger, Vec<LedgerOp>)> {
    let ops = store::ledger_ops(pool, pf).await?;
    let mut l = Ledger::new(method);
    for op in &ops {
        l.apply(op);
    }
    Ok((l, ops))
}

/// Today's price of an asset in the portfolio currency.
pub async fn price_of(pool: &PgPool, pf: &Portfolio, a: &Asset, today: Date) -> anyhow::Result<Option<f64>> {
    match a.last_price_usd {
        Some(p) => Ok(Some(
            otw_store::journal_fx::convert(pool, p, "USD", &pf.currency, today).await?.unwrap_or(p),
        )),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn d(y: i32, m: Month, day: u8) -> Date {
        Date::from_calendar_date(y, m, day).unwrap()
    }
    fn view(asset: Uuid, unreal: f64) -> LotView {
        LotView {
            asset_id: asset,
            symbol: "X".into(),
            opened: "2025-01-01".into(),
            qty: 1.0,
            cost: 100.0,
            cost_per_unit: 100.0,
            value: Some(100.0 + unreal),
            unrealized: Some(unreal),
            holding_days: 10,
            term: None,
        }
    }

    #[test]
    fn candidates_are_losers_worst_first() {
        let (a, b, c) = (Uuid::from_bytes([1; 16]), Uuid::from_bytes([2; 16]), Uuid::from_bytes([3; 16]));
        let today = d(2026, Month::June, 1);
        let out = harvest(
            vec![(view(a, -10.0), "stock".into()), (view(b, 25.0), "stock".into()), (view(c, -40.0), "stock".into())],
            &[],
            None,
            today,
        );
        assert_eq!(out.len(), 2, "a winning lot is not a candidate");
        assert_eq!(out[0].lot.asset_id, c);
        assert_eq!(out[1].lot.asset_id, a);
    }

    #[test]
    fn a_buy_inside_the_window_flags_the_lot_and_one_outside_does_not() {
        let a = Uuid::from_bytes([1; 16]);
        let today = d(2026, Month::June, 1);
        let w = WashWindow { before_days: 30, after_days: 30, classes: vec!["stock".into()], ..Default::default() };
        let inside = harvest(vec![(view(a, -5.0), "stock".into())], &[(a, d(2026, Month::May, 15))], Some(&w), today);
        assert!(inside[0].wash_flag);
        let outside = harvest(vec![(view(a, -5.0), "stock".into())], &[(a, d(2026, Month::March, 1))], Some(&w), today);
        assert!(!outside[0].wash_flag);
        assert_eq!(outside[0].last_buy.as_deref(), Some("2026-03-01"));
        // A class the rule does not cover is never flagged.
        let crypto = harvest(vec![(view(a, -5.0), "crypto".into())], &[(a, d(2026, Month::May, 15))], Some(&w), today);
        assert!(!crypto[0].wash_flag);
    }

    #[test]
    fn a_regime_without_a_wash_rule_flags_nothing() {
        let a = Uuid::from_bytes([1; 16]);
        let today = d(2026, Month::June, 1);
        let out = harvest(vec![(view(a, -5.0), "stock".into())], &[(a, today)], None, today);
        assert!(!out[0].wash_flag);
    }

    #[test]
    fn a_month_window_counts_calendar_months() {
        let w = WashWindow { before_months: 2, after_months: 2, ..Default::default() };
        assert_eq!(w.start(d(2026, Month::April, 30)), d(2026, Month::February, 28));
        assert_eq!(w.end(d(2026, Month::December, 15)), d(2027, Month::February, 15));
    }

    /// A US-style regime makes a lot long-term past one year; without a profile there is no
    /// term at all, never a guessed one.
    #[test]
    fn the_long_short_cut_off_follows_the_regime() {
        let today = d(2026, Month::June, 1);
        let none = TaxRules::defaults();
        assert_eq!(none.term("stock", d(2020, Month::January, 1), today), None);
        if let Some((r, _)) = taxcalc::regime::resolve("us_federal", 2026) {
            let rules = TaxRules { method: Method::Fifo, profile: None, regime: Some(r.clone()), wash: None };
            assert_eq!(rules.term("stock", d(2026, Month::January, 2), today).as_deref(), Some("short"));
            assert_ne!(rules.term("stock", d(2024, Month::January, 2), today).as_deref(), Some("short"));
        }
    }
}
