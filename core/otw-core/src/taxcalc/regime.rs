//! Tax regimes as data.
//!
//! A regime is one country's rules for one tax year, written as parameters of the generic
//! steps the engine knows (`assess`): calendar, FX date, cost method, anti-wash rule,
//! netting pools, holding tiers, exemptions, allowances, inclusion shares, rate schedules,
//! surcharges. A country is a TOML file, never code: `regimes/<id>/<year>.toml`, with its
//! test cases beside it in `<year>.cases.toml`.
//!
//! The schema is closed (`deny_unknown_fields`): a rule the engine does not implement
//! cannot be written into a file and silently ignored. Files are compiled in and validated
//! at first use; the test suite (`cases.rs`) validates them all and runs every case.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Month};

use super::lots::Method;

/// Every regime file, compiled in. `cases.rs` fails if a file on disk is missing here.
pub const FILES: &[(&str, &str)] = &[
    ("fr_pfu/2025.toml", include_str!("regimes/fr_pfu/2025.toml")),
    ("fr_pfu/2026.toml", include_str!("regimes/fr_pfu/2026.toml")),
    ("fr_pro/2026.toml", include_str!("regimes/fr_pro/2026.toml")),
    ("de_abgeltung/2026.toml", include_str!("regimes/de_abgeltung/2026.toml")),
    ("uk_cgt/2025.toml", include_str!("regimes/uk_cgt/2025.toml")),
    ("uk_cgt/2026.toml", include_str!("regimes/uk_cgt/2026.toml")),
    ("us_federal/2025.toml", include_str!("regimes/us_federal/2025.toml")),
    ("us_federal/2026.toml", include_str!("regimes/us_federal/2026.toml")),
    ("ch_private/2026.toml", include_str!("regimes/ch_private/2026.toml")),
    ("es_ahorro/2026.toml", include_str!("regimes/es_ahorro/2026.toml")),
    ("it_sostitutiva/2025.toml", include_str!("regimes/it_sostitutiva/2025.toml")),
    ("it_sostitutiva/2026.toml", include_str!("regimes/it_sostitutiva/2026.toml")),
    ("au_resident/2026.toml", include_str!("regimes/au_resident/2026.toml")),
];

/// The id of the regime built from a profile's own fields rather than from a file.
pub const CUSTOM: &str = "custom_flat";

/// Gain buckets (signed, netted in pools) and income buckets (never negative).
pub const GAIN_BUCKETS: &[&str] = &["capital", "derivative", "crypto"];
pub const ALL_BUCKETS: &[&str] = &["capital", "derivative", "crypto", "dividends", "interest"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regime {
    pub id: String,
    /// The tax year this file describes (see `calendar.label` for how a year is named).
    pub year: i32,
    pub country: String,
    pub label: String,
    #[serde(default = "individual")]
    pub person_type: String,
    pub currency: String,
    /// complete: every rule the engine knows for this country is written. simple: headline
    /// rates only, the UI says so.
    pub coverage: Coverage,
    /// draft until a person checked every figure against the sources.
    pub status: Status,
    #[serde(default)]
    pub verified_on: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub notes: String,
    /// Optional form inputs this regime reads.
    #[serde(default)]
    pub inputs: Vec<InputKind>,
    #[serde(default)]
    pub calendar: Calendar,
    #[serde(default)]
    pub fx_date: FxDate,
    pub matching: Matching,
    #[serde(default)]
    pub pools: Vec<Pool>,
    #[serde(default)]
    pub holding: Vec<Holding>,
    #[serde(default)]
    pub exempt: Vec<Exempt>,
    #[serde(default)]
    pub allowances: Vec<Allowance>,
    #[serde(default)]
    pub inclusion: Vec<Inclusion>,
    pub schedules: Vec<Schedule>,
    #[serde(default)]
    pub surcharges: Vec<Surcharge>,
}

fn individual() -> String {
    "individual".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Simple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Draft,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// Taxable income outside this calculation, for schedules stacked on it.
    OtherIncome,
    /// The taxpayer's marginal income-tax rate, for `marginal` schedules.
    MarginalRate,
    /// The long-term share of each gain bucket, for holding tiers.
    LongTerm,
    /// Sale proceeds per bucket, for allowances measured on proceeds.
    Proceeds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    /// First day of the tax year, MM-DD.
    pub year_start: String,
    /// Whether tax year N is the period that starts in N or the one that ends in N.
    pub label: YearLabel,
}

impl Default for Calendar {
    fn default() -> Self {
        Self { year_start: "01-01".into(), label: YearLabel::Start }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YearLabel {
    Start,
    End,
}

/// Which day's rate converts a foreign-currency leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FxDate {
    #[default]
    SameDay,
    /// The last rate published before the transaction day (Poland, NBP D-1).
    PreviousDay,
    /// The last day of the month before the transaction (India, Rule 115).
    PreviousMonthEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matching {
    pub method: String,
    #[serde(default)]
    pub wash: Option<Wash>,
}

/// A loss followed (or preceded) by a repurchase of the same instrument inside the window
/// is deferred: it is added to the cost of the replacement shares.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wash {
    #[serde(default)]
    pub before_days: u32,
    #[serde(default)]
    pub after_days: u32,
    #[serde(default)]
    pub before_months: u32,
    #[serde(default)]
    pub after_months: u32,
    /// Asset classes the rule covers. Empty = all.
    #[serde(default)]
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub key: String,
    pub buckets: Vec<String>,
    /// Years a net loss survives. Absent = no limit, 0 = never carried.
    #[serde(default)]
    pub carry_years: Option<u32>,
    /// Carried losses only reduce the pool's gain down to its allowance (UK).
    #[serde(default)]
    pub preserve_allowance: bool,
    /// Pools this pool's leftover loss (this year's and carried) may also offset, one way
    /// (DE: the general pot offsets share gains, share losses never offset the general pot).
    #[serde(default)]
    pub spill_to: Vec<String>,
}

/// Gains in `bucket` held long enough take the term `term` (selector `bucket.term`): at
/// least `min_days`, or sold after the `after_years` anniversary of the purchase ("more
/// than one year": US, DE, AU). Exactly one of the two.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holding {
    pub bucket: String,
    #[serde(default)]
    pub min_days: Option<i64>,
    #[serde(default)]
    pub after_years: Option<u32>,
    pub term: String,
}

impl Holding {
    /// The threshold in days, for the form (which only knows a day count). Exact for
    /// `min_days`, the shortest qualifying span (365 × years + 1) for `after_years`.
    pub fn days(&self) -> i64 {
        self.min_days.unwrap_or_else(|| self.after_years.unwrap_or(0) as i64 * 365 + 1)
    }
    /// Does a lot bought on `opened` and sold on `closed` reach this tier?
    pub fn reached(&self, opened: Date, closed: Date) -> bool {
        match (self.min_days, self.after_years) {
            (Some(d), _) => (closed - opened).whole_days() >= d,
            (None, Some(y)) => {
                let y = opened.year() + y as i32;
                let anniversary = Date::from_calendar_date(y, opened.month(), opened.day())
                    .or_else(|_| Date::from_calendar_date(y, Month::March, 1))
                    .unwrap_or(opened);
                closed > anniversary
            }
            (None, None) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exempt {
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allowance {
    pub items: Vec<String>,
    pub amount: f64,
    pub kind: AllowanceKind,
    #[serde(default)]
    pub measure: Measure,
    /// A cliff that only exempts amounts strictly below it (DE: "weniger als 1 000 Euro").
    /// Default: at or under (FR: "n'excède pas 305 euros").
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowanceKind {
    /// Subtracted from the base (Freibetrag, AEA).
    Allowance,
    /// Nothing is due at or under the amount, everything above it (Freigrenze, FR 305).
    Cliff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    #[default]
    Gain,
    Proceeds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inclusion {
    pub items: Vec<String>,
    /// Share of the gain that is taxable (AU discount 0.5, CA inclusion 0.5).
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub key: String,
    pub label: String,
    pub items: Vec<String>,
    /// Stacking order: a stacked schedule sits on top of other income and of every stacked
    /// schedule with a lower order.
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub stack: bool,
    pub kind: ScheduleKind,
    /// Marginal brackets, ascending; the last has no `up_to`.
    #[serde(default)]
    pub brackets: Vec<Bracket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    Brackets,
    /// The taxpayer's own marginal rate (input `marginal_rate`).
    Marginal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bracket {
    #[serde(default)]
    pub up_to: Option<f64>,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surcharge {
    pub label: String,
    pub items: Vec<String>,
    pub rate: f64,
    pub on: SurchargeOn,
    /// Applies to the part of (other income + base) above this, capped at the base (NIIT).
    #[serde(default)]
    pub threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurchargeOn {
    /// A percentage of the items' taxable base (FR prélèvements sociaux, NIIT, Medicare).
    Base,
    /// A percentage of the tax the schedules put on those items (DE Soli).
    Tax,
}

impl Regime {
    pub fn method(&self) -> Method {
        Method::parse(&self.matching.method).unwrap_or(Method::Fifo)
    }

    /// The tax year as a [start, end] day pair.
    pub fn year_bounds(&self, year: i32) -> Result<(Date, Date), String> {
        let (m, d) = parse_md(&self.calendar.year_start)?;
        let start_year = match self.calendar.label {
            YearLabel::Start => year,
            YearLabel::End if (m, d) == (1, 1) => year,
            YearLabel::End => year - 1,
        };
        let start = Date::from_calendar_date(start_year, month(m)?, d)
            .map_err(|_| format!("{year} has no {}", self.calendar.year_start))?;
        let next = Date::from_calendar_date(start_year + 1, month(m)?, d)
            .map_err(|_| format!("{} has no {}", start_year + 1, self.calendar.year_start))?;
        Ok((start, next.previous_day().expect("not the first day of time")))
    }

    /// The day whose rate converts a leg traded on `day`.
    pub fn fx_day(&self, day: Date) -> Date {
        match self.fx_date {
            FxDate::SameDay => day,
            FxDate::PreviousDay => day.previous_day().unwrap_or(day),
            FxDate::PreviousMonthEnd => day.replace_day(1).ok().and_then(|d| d.previous_day()).unwrap_or(day),
        }
    }

    /// The shortest holding tier of a bucket: at or past it, a gain is long-term.
    pub fn long_after(&self, bucket: &str) -> Option<i64> {
        self.holding.iter().filter(|h| h.bucket == bucket).map(Holding::days).min()
    }

    /// The term of a lot bought on `opened` and sold on `closed`: the longest tier reached.
    pub fn term_between(&self, bucket: &str, opened: Date, closed: Date) -> String {
        self.holding
            .iter()
            .filter(|h| h.bucket == bucket && h.reached(opened, closed))
            .max_by_key(|h| h.days())
            .map(|h| h.term.clone())
            .unwrap_or_else(|| "short".into())
    }

    /// Every item this regime can see: each gain bucket short-term and in each of its
    /// holding terms, each income bucket once.
    pub fn items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        for b in ALL_BUCKETS {
            if !GAIN_BUCKETS.contains(b) {
                out.push(Item { bucket: b.to_string(), term: None });
                continue;
            }
            out.push(Item { bucket: b.to_string(), term: Some("short".into()) });
            for h in self.holding.iter().filter(|h| h.bucket == *b) {
                let it = Item { bucket: b.to_string(), term: Some(h.term.clone()) };
                if !out.contains(&it) {
                    out.push(it);
                }
            }
        }
        out
    }

    /// The term of a gain in `bucket` held `days`: the longest tier reached, else short.
    pub fn term_for(&self, bucket: &str, days: i64) -> String {
        self.holding
            .iter()
            .filter(|h| h.bucket == bucket && days >= h.days())
            .max_by_key(|h| h.days())
            .map(|h| h.term.clone())
            .unwrap_or_else(|| "short".into())
    }

    pub fn reads(&self, k: InputKind) -> bool {
        self.inputs.contains(&k)
    }

    /// Everything that must hold for the engine to read this file the way it was meant.
    pub fn validate(&self) -> Result<(), String> {
        let bad = |m: String| Err(format!("{} {}: {m}", self.id, self.year));
        if self.sources.is_empty() {
            return bad("no source".into());
        }
        if self.status == Status::Verified && parse_ymd(&self.verified_on).is_none() {
            return bad("verified without a verified_on date".into());
        }
        if Method::parse(&self.matching.method).is_none() {
            return bad(format!("unknown method {}", self.matching.method));
        }
        if let Err(e) = parse_md(&self.calendar.year_start) {
            return bad(e);
        }
        let terms: Vec<String> = self.holding.iter().map(|h| h.term.clone()).collect();
        let check = |sel: &str| -> Result<(), String> {
            let (b, term) = split(sel);
            if !ALL_BUCKETS.contains(&b) {
                return Err(format!("unknown bucket in {sel}"));
            }
            if let Some(t) = term {
                if t != "short" && !terms.iter().any(|x| x == t) {
                    return Err(format!("unknown term in {sel}"));
                }
            }
            Ok(())
        };
        let selectors = self
            .exempt
            .iter()
            .flat_map(|e| e.items.iter())
            .chain(self.allowances.iter().flat_map(|a| a.items.iter()))
            .chain(self.inclusion.iter().flat_map(|a| a.items.iter()))
            .chain(self.schedules.iter().flat_map(|s| s.items.iter()))
            .chain(self.surcharges.iter().flat_map(|s| s.items.iter()));
        for s in selectors {
            check(s).or_else(|e| bad(e))?;
        }
        for p in &self.pools {
            for b in &p.buckets {
                if !ALL_BUCKETS.contains(&b.as_str()) {
                    return bad(format!("pool {} holds {b}, not a bucket", p.key));
                }
            }
            for t in &p.spill_to {
                if !self.pools.iter().any(|q| q.key == *t && q.key != p.key) {
                    return bad(format!("pool {} spills to unknown pool {t}", p.key));
                }
            }
        }
        for h in &self.holding {
            if !GAIN_BUCKETS.contains(&h.bucket.as_str()) || h.term == "short" {
                return bad(format!("bad holding tier {}.{}", h.bucket, h.term));
            }
            if h.min_days.is_some() == h.after_years.is_some() {
                return bad(format!("holding tier {}.{}: set min_days or after_years", h.bucket, h.term));
            }
        }
        for s in &self.schedules {
            if s.kind == ScheduleKind::Brackets {
                let n = s.brackets.len();
                if n == 0 || s.brackets[n - 1].up_to.is_some() {
                    return bad(format!("schedule {}: the last bracket must be open", s.key));
                }
                if s.brackets.windows(2).any(|w| w[0].up_to >= w[1].up_to.or(Some(f64::INFINITY))) {
                    return bad(format!("schedule {}: brackets not ascending", s.key));
                }
            } else if !self.reads(InputKind::MarginalRate) {
                return bad(format!("schedule {} is marginal but marginal_rate is not an input", s.key));
            }
            if s.stack && !self.reads(InputKind::OtherIncome) {
                return bad(format!("schedule {} stacks but other_income is not an input", s.key));
            }
        }
        // Every taxable item must land in exactly one schedule, or be exempt: a bucket that
        // falls through would be untaxed in silence.
        for item in self.items() {
            if self.exempt.iter().any(|e| e.items.iter().any(|s| item.matches(s))) {
                continue;
            }
            let n = self.schedules.iter().filter(|s| s.items.iter().any(|sel| item.matches(sel))).count();
            if n != 1 {
                return bad(format!("{} is in {n} schedules, expected 1", item.name()));
            }
        }
        Ok(())
    }
}

/// An item: a bucket and, when the regime splits it by holding period, a term.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Item {
    pub bucket: String,
    pub term: Option<String>,
}

impl Item {
    pub fn name(&self) -> String {
        match &self.term {
            Some(t) => format!("{}.{t}", self.bucket),
            None => self.bucket.clone(),
        }
    }
    /// `capital` matches every term of the bucket, `capital.long` only that term.
    pub fn matches(&self, sel: &str) -> bool {
        let (b, t) = split(sel);
        b == self.bucket && t.is_none_or(|t| self.term.as_deref() == Some(t))
    }
}

fn split(sel: &str) -> (&str, Option<&str>) {
    match sel.split_once('.') {
        Some((b, t)) => (b, Some(t)),
        None => (sel, None),
    }
}

fn parse_md(s: &str) -> Result<(u8, u8), String> {
    let (m, d) = s.split_once('-').ok_or(format!("{s} is not MM-DD"))?;
    let m: u8 = m.parse().map_err(|_| format!("{s} is not MM-DD"))?;
    let d: u8 = d.parse().map_err(|_| format!("{s} is not MM-DD"))?;
    month(m)?;
    Ok((m, d))
}

fn month(m: u8) -> Result<Month, String> {
    Month::try_from(m).map_err(|_| format!("{m} is not a month"))
}

pub fn parse_ymd(s: &str) -> Option<Date> {
    Date::parse(s, &time::format_description::well_known::Iso8601::DATE).ok()
}

// ── Registry ────────────────────────────────────────────────────────────────────────────

fn registry() -> &'static BTreeMap<String, Vec<Regime>> {
    static REG: OnceLock<BTreeMap<String, Vec<Regime>>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut out: BTreeMap<String, Vec<Regime>> = BTreeMap::new();
        for (path, src) in FILES {
            // A broken file is a build defect: the case suite fails on it before release.
            // At runtime it is skipped rather than taking the whole module down.
            match load(path, src) {
                Ok(r) => out.entry(r.id.clone()).or_default().push(r),
                Err(e) => tracing::error!("tax regime {path}: {e}"),
            }
        }
        for v in out.values_mut() {
            v.sort_by_key(|r| r.year);
        }
        out
    })
}

pub fn load(path: &str, src: &str) -> Result<Regime, String> {
    let r: Regime = toml::from_str(src).map_err(|e| e.to_string())?;
    let expected = format!("{}/{}.toml", r.id, r.year);
    if path != expected {
        return Err(format!("{path} declares {expected}"));
    }
    r.validate()?;
    Ok(r)
}

/// Every regime id with the years it has files for.
pub fn catalog() -> Vec<&'static Regime> {
    registry().values().filter_map(|v| v.last()).collect()
}

pub fn years_of(id: &str) -> Vec<i32> {
    registry().get(id).map(|v| v.iter().map(|r| r.year).collect()).unwrap_or_default()
}

/// The file for `year`: the latest one at or before it, else the earliest (with a warning
/// from the caller, since rules for a year before the first file are unknown).
pub fn resolve(id: &str, year: i32) -> Option<(&'static Regime, bool)> {
    let v = registry().get(id)?;
    match v.iter().rev().find(|r| r.year <= year) {
        Some(r) => Some((r, r.year == year)),
        None => v.first().map(|r| (r, false)),
    }
}

// ── The custom regime, built from a profile's own fields ────────────────────────────────

/// A profile on `custom_flat` describes its rules itself: one flat (or income + social)
/// rate, optional holding relief tiers, two allowances, one pool.
pub fn custom(profile: &Value, context: &str) -> Regime {
    let f = |k: &str| profile.get(k).and_then(Value::as_f64);
    let flat = f("flat_rate");
    let marginal = f("marginal_income_rate");
    let social = f("social_charges_rate").unwrap_or(0.0);
    let allowance = |k: &str| {
        profile
            .get("allowances")
            .and_then(|a| a.get(k))
            .and_then(|v| v.get("annual_free"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    let mut tiers: Vec<(i64, f64)> = profile
        .get("holding_period_rules")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| Some((r.get("min_days")?.as_i64()?, r.get("rate")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default();
    tiers.sort_by_key(|t| t.0);
    let carry_years = profile
        .get("loss_carry")
        .and_then(|l| l.get("years"))
        .and_then(Value::as_u64)
        .map(|y| y as u32);

    // The rate on a gain with no relief: trading favours the income rate, investing the flat.
    let base_rate = if context == "trading" {
        marginal.map(|m| m + social).or(flat).unwrap_or(0.0)
    } else if !tiers.is_empty() {
        marginal.map(|m| m + social).or(flat).unwrap_or(0.0)
    } else {
        flat.or(marginal.map(|m| m + social)).unwrap_or(0.0)
    };
    let income_rate = flat.or(marginal).unwrap_or(0.0);
    let flat_schedule = |key: &str, label: &str, items: Vec<String>, rate: f64| Schedule {
        key: key.into(),
        label: label.into(),
        items,
        order: 0,
        stack: false,
        kind: ScheduleKind::Brackets,
        brackets: vec![Bracket { up_to: None, rate }],
    };
    let mut holding = Vec::new();
    let mut schedules = vec![
        flat_schedule("gains", "Gains", GAIN_BUCKETS.iter().map(|b| format!("{b}.short")).collect(), base_rate),
        flat_schedule("income", "Dividends and interest", vec!["dividends".into(), "interest".into()], income_rate),
    ];
    if context == "investing" {
        for (days, rate) in &tiers {
            let term = format!("{days}d");
            for b in GAIN_BUCKETS {
                holding.push(Holding { bucket: b.to_string(), min_days: Some(*days), after_years: None, term: term.clone() });
            }
            schedules.push(flat_schedule(
                &term,
                &format!("Held {days}+ days"),
                GAIN_BUCKETS.iter().map(|b| format!("{b}.{term}")).collect(),
                rate + social,
            ));
        }
    }
    Regime {
        id: CUSTOM.into(),
        year: 0,
        country: profile.get("country").and_then(Value::as_str).unwrap_or("").into(),
        label: "Custom rates".into(),
        person_type: "individual".into(),
        currency: profile.get("currency").and_then(Value::as_str).unwrap_or("USD").into(),
        coverage: Coverage::Simple,
        status: Status::Draft,
        verified_on: String::new(),
        sources: vec!["profile".into()],
        notes: String::new(),
        inputs: vec![InputKind::LongTerm, InputKind::Proceeds],
        calendar: Calendar::default(),
        fx_date: FxDate::SameDay,
        matching: Matching { method: "fifo".into(), wash: None },
        pools: vec![Pool {
            key: "all".into(),
            buckets: GAIN_BUCKETS.iter().map(|b| b.to_string()).collect(),
            carry_years,
            preserve_allowance: false,
            spill_to: vec![],
        }],
        holding,
        exempt: vec![],
        allowances: [("capital", allowance("capital_gains")), ("dividends", allowance("dividends"))]
            .into_iter()
            .filter(|(_, a)| *a > 0.0)
            .map(|(b, a)| Allowance {
                items: vec![b.into()],
                amount: a,
                kind: AllowanceKind::Allowance,
                measure: Measure::Gain,
                strict: false,
            })
            .collect(),
        inclusion: vec![],
        schedules,
        surcharges: vec![],
    }
}

/// The regime a profile computes under for `year`, and whether a file for that exact year
/// exists. Unknown ids fall back to the custom regime built from the profile.
pub fn for_profile(profile: &Value, context: &str, year: i32) -> (Regime, Option<String>) {
    let id = profile.get("regime").and_then(Value::as_str).unwrap_or(CUSTOM);
    if id != CUSTOM {
        if let Some((r, exact)) = resolve(id, year) {
            let warn = (!exact).then(|| {
                format!("No {id} rules for {year}: the {} rules were applied.", r.year)
            });
            return (r.clone(), warn);
        }
    }
    (custom(profile, context), None)
}
