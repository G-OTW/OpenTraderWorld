//! Storage + analytics for the Subscription Tracker module.
//!
//! Subscriptions bill on a cadence (weekly/monthly/quarterly/yearly); the breakdown
//! normalises each to a monthly-equivalent and converts into a display currency via the
//! shared `journal_fx` rates. Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

pub const FREQUENCIES: [&str; 4] = ["weekly", "monthly", "quarterly", "yearly"];

/// Factor converting one billing period's price into a monthly-equivalent amount.
fn monthly_factor(frequency: &str) -> f64 {
    match frequency {
        "weekly" => 52.0 / 12.0,
        "quarterly" => 1.0 / 3.0,
        "yearly" => 1.0 / 12.0,
        _ => 1.0, // monthly
    }
}

/// Number of whole months between two dates (a..b), for billing-month checks.
fn months_between(from: Date, to: Date) -> i32 {
    (to.year() - from.year()) * 12 + (to.month() as i32 - from.month() as i32)
}

#[derive(Debug, Clone, Serialize)]
pub struct Subscription {
    pub id: Uuid,
    pub name: String,
    pub platform: Option<String>,
    pub url: Option<String>,
    pub price: f64,
    pub currency: String,
    pub frequency: String,
    pub category: Option<String>,
    #[serde(with = "date_opt")]
    pub started_on: Option<Date>,
    pub active: bool,
}

mod date_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct SubscriptionInput {
    #[serde(default)]
    pub name: String,
    pub platform: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub price: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_frequency")]
    pub frequency: String,
    pub category: Option<String>,
    /// `YYYY-MM-DD`, or null/empty for no anchor.
    pub started_on: Option<String>,
    #[serde(default = "default_true")]
    pub active: bool,
}

impl SubscriptionInput {
    fn started_date(&self) -> Option<Date> {
        self.started_on
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
    }
}

fn default_currency() -> String {
    "USD".to_string()
}
fn default_frequency() -> String {
    "monthly".to_string()
}
fn default_true() -> bool {
    true
}

#[derive(sqlx::FromRow)]
struct SubRow {
    id: Uuid,
    name: String,
    platform: Option<String>,
    url: Option<String>,
    price: f64,
    currency: String,
    frequency: String,
    category: Option<String>,
    started_on: Option<Date>,
    active: bool,
}

fn row_to_sub(r: SubRow) -> Subscription {
    Subscription {
        id: r.id,
        name: r.name,
        platform: r.platform,
        url: r.url,
        price: r.price,
        currency: r.currency,
        frequency: r.frequency,
        category: r.category,
        started_on: r.started_on,
        active: r.active,
    }
}

const SUB_COLUMNS: &str =
    "id, name, platform, url, price, currency, frequency, category, started_on, active";

// ── CRUD ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct SubFilter {
    pub platform: Option<String>,
    pub category: Option<String>,
    /// Only active subscriptions when true (default shows all).
    pub active_only: bool,
}

pub async fn list_subscriptions(pool: &PgPool, filter: &SubFilter) -> anyhow::Result<Vec<Subscription>> {
    let sql = format!(
        "SELECT {SUB_COLUMNS} FROM subscriptions \
         WHERE ($1::text IS NULL OR platform = $1) \
           AND ($2::text IS NULL OR category = $2) \
           AND (NOT $3 OR active) \
         ORDER BY name, created_at"
    );
    let rows = sqlx::query_as::<_, SubRow>(sqlx::AssertSqlSafe(sql))
        .bind(filter.platform.as_deref().filter(|s| !s.is_empty()))
        .bind(filter.category.as_deref().filter(|s| !s.is_empty()))
        .bind(filter.active_only)
        .fetch_all(pool)
        .await
        .context("listing subscriptions")?;
    Ok(rows.into_iter().map(row_to_sub).collect())
}

pub async fn get_subscription(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Subscription>> {
    let sql = format!("SELECT {SUB_COLUMNS} FROM subscriptions WHERE id = $1");
    let row = sqlx::query_as::<_, SubRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching subscription")?;
    Ok(row.map(row_to_sub))
}

pub async fn add_subscription(pool: &PgPool, input: &SubscriptionInput) -> anyhow::Result<Subscription> {
    let id = Uuid::new_v4();
    let started = input.started_date();
    sqlx::query(
        "INSERT INTO subscriptions \
            (id, name, platform, url, price, currency, frequency, category, started_on, active) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.platform.as_deref())
    .bind(input.url.as_deref())
    .bind(input.price)
    .bind(&input.currency)
    .bind(&input.frequency)
    .bind(input.category.as_deref())
    .bind(started)
    .bind(input.active)
    .execute(pool)
    .await
    .context("inserting subscription")?;
    get_subscription(pool, id)
        .await?
        .context("subscription vanished after insert")
}

pub async fn update_subscription(
    pool: &PgPool,
    id: Uuid,
    input: &SubscriptionInput,
) -> anyhow::Result<bool> {
    let started = input.started_date();
    let res = sqlx::query(
        "UPDATE subscriptions SET \
            name = $2, platform = $3, url = $4, price = $5, currency = $6, frequency = $7, \
            category = $8, started_on = $9, active = $10, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.platform.as_deref())
    .bind(input.url.as_deref())
    .bind(input.price)
    .bind(&input.currency)
    .bind(&input.frequency)
    .bind(input.category.as_deref())
    .bind(started)
    .bind(input.active)
    .execute(pool)
    .await
    .context("updating subscription")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_subscription(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM subscriptions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Autocomplete suggestions ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SubSuggestions {
    pub platforms: Vec<String>,
    pub categories: Vec<String>,
}

async fn distinct(pool: &PgPool, column: &str) -> anyhow::Result<Vec<String>> {
    // `column` is a fixed internal identifier (never user input).
    let sql = format!(
        "SELECT DISTINCT {column} FROM subscriptions \
         WHERE {column} IS NOT NULL AND {column} <> '' ORDER BY {column}"
    );
    let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .with_context(|| format!("listing distinct {column}"))?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

pub async fn suggestions(pool: &PgPool) -> anyhow::Result<SubSuggestions> {
    Ok(SubSuggestions {
        platforms: distinct(pool, "platform").await?,
        categories: distinct(pool, "category").await?,
    })
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Settings {
    pub display_currency: String,
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT display_currency FROM subscription_settings WHERE id = TRUE")
            .fetch_optional(pool)
            .await
            .context("loading subscription settings")?;
    Ok(Settings {
        display_currency: row.map(|r| r.0).unwrap_or_else(|| "USD".to_string()),
    })
}

pub async fn set_display_currency(pool: &PgPool, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO subscription_settings (id, display_currency, updated_at) \
         VALUES (TRUE, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET display_currency = $1, updated_at = now()",
    )
    .bind(currency)
    .execute(pool)
    .await
    .context("setting subscription display currency")?;
    Ok(())
}

// ── Breakdown ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MonthPoint {
    /// First day of the month (YYYY-MM-01) as ISO date.
    pub month: String,
    pub amount: f64,
    /// Per-subscription contribution to this month's billed amount, keyed by sub id (string).
    /// Lets the chart stack one colored segment per subscription.
    pub by_sub: std::collections::HashMap<String, f64>,
    /// Per-category contribution to this month's billed amount, keyed by category name.
    pub by_category: std::collections::HashMap<String, f64>,
}

/// A subscription's identity for the chart legend / stacking order.
#[derive(Debug, Serialize)]
pub struct SubLegend {
    pub id: String,
    pub name: String,
}

/// Monthly-equivalent recurring spend grouped by category (converted to display currency).
#[derive(Debug, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub amount: f64,
}

#[derive(Debug, Serialize)]
pub struct Breakdown {
    pub display_currency: String,
    /// Monthly-equivalent recurring spend (all active subs), converted.
    pub monthly_total: f64,
    /// Yearly-equivalent (monthly_total × 12).
    pub yearly_total: f64,
    /// Amount actually billed next calendar month (frequency + start-date aware).
    pub next_month_total: f64,
    /// Per-month columns for the chart (monthly-equivalent recurring spend).
    pub months: Vec<MonthPoint>,
    /// Per-calendar-year columns: every active sub's FULL annual cost (yearly-equivalent),
    /// counted whole for any year it's active — so future years aren't understated by a
    /// window that doesn't span the whole year. `month` holds `YYYY-01-01`.
    pub years: Vec<MonthPoint>,
    /// Subscriptions contributing to the breakdown (stable legend / stacking order).
    pub subs: Vec<SubLegend>,
    /// Monthly-equivalent spend per category, ranked high→low.
    pub categories: Vec<CategoryTotal>,
    pub active_count: i64,
    /// Subs whose currency could not be converted (no FX rate) — excluded from totals.
    pub unconverted: i64,
}

/// Whether a subscription bills in the calendar month containing `target` (first-of-month),
/// given its frequency and start date. Subs with no start date are treated as billing every
/// applicable period anchored to the current month.
fn bills_in_month(sub: &Subscription, target: Date, anchor: Date) -> bool {
    let start = sub.started_on.unwrap_or(anchor);
    if start > target {
        return false;
    }
    let n = months_between(start, target);
    match sub.frequency.as_str() {
        "monthly" => true,
        "quarterly" => n % 3 == 0,
        "yearly" => n % 12 == 0,
        // Weekly bills every month.
        "weekly" => true,
        _ => true,
    }
}

/// Build the breakdown over `months_back..months_fwd` around the current month, converting
/// every amount into `display_currency` at today's rate.
pub async fn breakdown(
    pool: &PgPool,
    filter: &SubFilter,
    display_currency: &str,
    months_back: i64,
    months_fwd: i64,
) -> anyhow::Result<Breakdown> {
    // Only active subs contribute to recurring spend.
    let mut f = SubFilter {
        platform: filter.platform.clone(),
        category: filter.category.clone(),
        active_only: true,
    };
    f.active_only = true;
    let subs = list_subscriptions(pool, &f).await?;

    let today = OffsetDateTime::now_utc().date();
    let this_month = today.replace_day(1).unwrap_or(today);

    // Monthly-equivalent recurring total (converted).
    let mut monthly_total = 0.0;
    let mut unconverted = 0i64;
    let mut by_category: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for s in &subs {
        let monthly = s.price * monthly_factor(&s.frequency);
        match crate::journal_fx::convert(pool, monthly, &s.currency, display_currency, today).await? {
            Some(v) => {
                monthly_total += v;
                let cat = s
                    .category
                    .as_deref()
                    .filter(|c| !c.is_empty())
                    .unwrap_or("Uncategorized");
                *by_category.entry(cat.to_string()).or_insert(0.0) += v;
            }
            None => unconverted += 1,
        }
    }
    let mut categories: Vec<CategoryTotal> = by_category
        .into_iter()
        .map(|(category, amount)| CategoryTotal { category, amount })
        .collect();
    categories.sort_by(|a, b| {
        b.amount
            .partial_cmp(&a.amount)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.category.cmp(&b.category))
    });

    // Per-month columns: the amount actually billed in each calendar month (frequency +
    // start-date aware), so yearly/quarterly subs spike in their billing month rather than
    // being smoothed flat.
    let mut months = Vec::new();
    for off in -months_back..=months_fwd {
        let m = add_months(this_month, off);
        let mut amount = 0.0;
        let mut by_sub = std::collections::HashMap::new();
        // `by_category` uses the monthly-EQUIVALENT (weekly ×52/12, monthly ×1, quarterly ÷3,
        // yearly ÷12) for every sub active that month, so category bars are smoothed/comparable
        // rather than spiking in a billing month (unlike `amount`/`by_sub`, which are billed).
        let mut by_category: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for s in &subs {
            // Billed-this-month amount feeds the spend bars (frequency + start aware).
            if bills_in_month(s, m, this_month) {
                if let Some(v) =
                    crate::journal_fx::convert(pool, s.price, &s.currency, display_currency, today)
                        .await?
                {
                    amount += v;
                    by_sub.insert(s.id.to_string(), v);
                }
            }
            // Monthly-equivalent contribution for the category view: any month on/after the
            // sub's start (or always, if no start anchor).
            if s.started_on.map(|d| d <= m).unwrap_or(true) {
                let monthly = s.price * monthly_factor(&s.frequency);
                if let Some(v) =
                    crate::journal_fx::convert(pool, monthly, &s.currency, display_currency, today)
                        .await?
                {
                    let cat = s
                        .category
                        .as_deref()
                        .filter(|c| !c.is_empty())
                        .unwrap_or("Uncategorized");
                    *by_category.entry(cat.to_string()).or_insert(0.0) += v;
                }
            }
        }
        months.push(MonthPoint {
            month: format_month(m),
            amount,
            by_sub,
            by_category,
        });
    }

    // Per-calendar-year columns. Each active sub contributes its FULL annual cost
    // (yearly-equivalent = monthly-equivalent × 12) to every year it's active — start anchor
    // aware, but never prorated — so a future year shows the complete spend, not a partial
    // sum of whatever months happen to fall inside the window.
    let first_year = add_months(this_month, -months_back).year();
    let last_year = add_months(this_month, months_fwd).year();
    let mut years = Vec::new();
    for y in first_year..=last_year {
        let year_end =
            Date::from_calendar_date(y, time::Month::December, 31).unwrap_or(this_month);
        let mut amount = 0.0;
        let mut by_sub = std::collections::HashMap::new();
        let mut by_category: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for s in &subs {
            // Active in year `y` if it has started by the end of that year (or no anchor).
            if s.started_on.map(|d| d <= year_end).unwrap_or(true) {
                let yearly = s.price * monthly_factor(&s.frequency) * 12.0;
                if let Some(v) =
                    crate::journal_fx::convert(pool, yearly, &s.currency, display_currency, today)
                        .await?
                {
                    amount += v;
                    by_sub.insert(s.id.to_string(), v);
                    let cat = s
                        .category
                        .as_deref()
                        .filter(|c| !c.is_empty())
                        .unwrap_or("Uncategorized");
                    *by_category.entry(cat.to_string()).or_insert(0.0) += v;
                }
            }
        }
        years.push(MonthPoint {
            month: format!("{y:04}-01-01"),
            amount,
            by_sub,
            by_category,
        });
    }

    // Next calendar month's actual billed amount (frequency + start aware), converted.
    let next_month = add_months(this_month, 1);
    let mut next_month_total = 0.0;
    for s in &subs {
        if bills_in_month(s, next_month, this_month) {
            if let Some(v) =
                crate::journal_fx::convert(pool, s.price, &s.currency, display_currency, today).await?
            {
                next_month_total += v;
            }
        }
    }

    Ok(Breakdown {
        display_currency: display_currency.to_string(),
        monthly_total,
        yearly_total: monthly_total * 12.0,
        next_month_total,
        months,
        years,
        subs: subs
            .iter()
            .map(|s| SubLegend {
                id: s.id.to_string(),
                name: s.name.clone(),
            })
            .collect(),
        categories,
        active_count: subs.len() as i64,
        unconverted,
    })
}

/// Add `n` months to a first-of-month date (n may be negative).
fn add_months(d: Date, n: i64) -> Date {
    let total = (d.year() as i64) * 12 + (d.month() as i64 - 1) + n;
    let year = (total.div_euclid(12)) as i32;
    let month0 = total.rem_euclid(12) as u8; // 0..11
    let month = time::Month::try_from(month0 + 1).unwrap_or(time::Month::January);
    Date::from_calendar_date(year, month, 1).unwrap_or(d)
}

fn format_month(d: Date) -> String {
    format!("{:04}-{:02}-01", d.year(), d.month() as u8)
}
