//! Storage + analytics for the Subscription Tracker module.
//!
//! Subscriptions bill on a cadence (weekly/monthly/quarterly/yearly); the breakdown
//! normalises each to a monthly-equivalent and converts into a display currency via the
//! shared `journal_fx` rates. Single-user: no owner scoping.
//!
//! Every total uses the user's own share of a plan (`my_share_pct`) and leaves out a
//! cancelled service from its end date (`ended_at`). Billing dates are anchored on the
//! trial end when there is one, else on the start date.

use std::collections::{BTreeMap, HashMap};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::journal_fx::FxCache;

pub const FREQUENCIES: [&str; 4] = ["weekly", "monthly", "quarterly", "yearly"];

/// Factor converting one billing period's price into a monthly-equivalent amount.
pub fn monthly_factor(frequency: &str) -> f64 {
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
    /// Days before a billing date to notify; null = no reminder.
    pub remind_days: Option<i32>,
    /// Channel ids for the reminder; empty = every channel granted to Subscriptions.
    pub remind_channels: Vec<Uuid>,
    #[serde(with = "date_opt")]
    pub reminded_for: Option<Date>,
    #[serde(with = "date_opt")]
    pub trial_end: Option<Date>,
    #[serde(with = "date_opt")]
    pub ended_at: Option<Date>,
    pub yearly_price: Option<f64>,
    pub payment_method: Option<String>,
    /// `YYYY-MM`.
    pub card_expiry: Option<String>,
    pub my_share_pct: f64,
    /// Latest usage rating (1 to 5) and its date.
    pub last_score: Option<i16>,
    #[serde(with = "date_opt")]
    pub last_rated_at: Option<Date>,
}

impl Subscription {
    /// The date billing dates repeat from: the trial end when set, else the start date.
    pub fn anchor(&self) -> Option<Date> {
        self.trial_end.or(self.started_on)
    }

    /// The user's part of a plan, as a factor (50 % = 0.5).
    pub fn share(&self) -> f64 {
        share_factor(self.my_share_pct)
    }

    /// Monthly-equivalent cost to the user, in the subscription's own currency.
    pub fn monthly_equivalent(&self) -> f64 {
        self.price * monthly_factor(&self.frequency) * self.share()
    }

    /// Cancelled on or before `day`.
    pub fn ended_by(&self, day: Date) -> bool {
        self.ended_at.is_some_and(|e| e <= day)
    }
}

/// `my_share_pct` as a factor, clamped to (0, 100] so a bad row never inflates a total.
pub fn share_factor(pct: f64) -> f64 {
    if pct.is_finite() && pct > 0.0 {
        pct.min(100.0) / 100.0
    } else {
        1.0
    }
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

pub fn iso_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
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
    /// Days before each billing date to notify (0 to 60); null = off.
    pub remind_days: Option<i32>,
    /// Channel ids for the reminder; empty = every channel granted to Subscriptions.
    #[serde(default)]
    pub remind_channels: Vec<Uuid>,
    /// `YYYY-MM-DD` the free trial converts to paid; null = no trial.
    pub trial_end: Option<String>,
    /// `YYYY-MM-DD` the service was cancelled; null = still running.
    pub ended_at: Option<String>,
    /// Price of the yearly plan, for a plan billed more often.
    pub yearly_price: Option<f64>,
    pub payment_method: Option<String>,
    /// `YYYY-MM`.
    pub card_expiry: Option<String>,
    /// The user's part of the price in percent (family plans), 100 by default.
    #[serde(default = "default_share")]
    pub my_share_pct: f64,
}

fn parse_opt_date(s: &Option<String>) -> Option<Date> {
    s.as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| crate::journal_fx::parse_date(s).ok())
}

impl SubscriptionInput {
    pub fn started_date(&self) -> Option<Date> {
        parse_opt_date(&self.started_on)
    }
    pub fn trial_date(&self) -> Option<Date> {
        parse_opt_date(&self.trial_end)
    }
    pub fn ended_date(&self) -> Option<Date> {
        parse_opt_date(&self.ended_at)
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
fn default_share() -> f64 {
    100.0
}

/// A valid `YYYY-MM` card expiry.
pub fn valid_card_expiry(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..].iter().all(u8::is_ascii_digit)
        && matches!(s[5..].parse::<u8>(), Ok(1..=12))
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
    remind_days: Option<i32>,
    remind_channels: serde_json::Value,
    reminded_for: Option<Date>,
    trial_end: Option<Date>,
    ended_at: Option<Date>,
    yearly_price: Option<f64>,
    payment_method: Option<String>,
    card_expiry: Option<String>,
    my_share_pct: f64,
    last_score: Option<i16>,
    last_rated_at: Option<Date>,
}

/// Channel ids out of a JSON array; anything else reads as "every granted channel".
pub fn channel_ids(v: &serde_json::Value) -> Vec<Uuid> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn ids_json(ids: &[Uuid]) -> serde_json::Value {
    serde_json::Value::Array(ids.iter().map(|i| serde_json::Value::String(i.to_string())).collect())
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
        remind_days: r.remind_days,
        remind_channels: channel_ids(&r.remind_channels),
        reminded_for: r.reminded_for,
        trial_end: r.trial_end,
        ended_at: r.ended_at,
        yearly_price: r.yearly_price,
        payment_method: r.payment_method,
        card_expiry: r.card_expiry,
        my_share_pct: r.my_share_pct,
        last_score: r.last_score,
        last_rated_at: r.last_rated_at,
    }
}

const SUB_COLUMNS: &str = "s.id, s.name, s.platform, s.url, s.price, s.currency, s.frequency, \
     s.category, s.started_on, s.active, s.remind_days, s.remind_channels, s.reminded_for, \
     s.trial_end, s.ended_at, s.yearly_price, s.payment_method, s.card_expiry, s.my_share_pct, \
     r.score AS last_score, r.rated_at AS last_rated_at";

/// The latest rating of each subscription, joined beside its row.
const SUB_FROM: &str = "subscriptions s LEFT JOIN LATERAL ( \
         SELECT score, rated_at FROM subscription_ratings \
         WHERE subscription_id = s.id ORDER BY rated_at DESC, created_at DESC LIMIT 1 \
     ) r ON TRUE";

// ── CRUD ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct SubFilter {
    pub platform: Option<String>,
    pub category: Option<String>,
    /// Only active subscriptions when true (default shows all). Ended rows stay in: the
    /// breakdown needs their past months, callers that must skip them test `ended_by`.
    pub active_only: bool,
}

pub async fn list_subscriptions(pool: &PgPool, filter: &SubFilter) -> anyhow::Result<Vec<Subscription>> {
    let sql = format!(
        "SELECT {SUB_COLUMNS} FROM {SUB_FROM} \
         WHERE ($1::text IS NULL OR s.platform = $1) \
           AND ($2::text IS NULL OR s.category = $2) \
           AND (NOT $3 OR s.active) \
         ORDER BY s.name, s.created_at"
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
    let sql = format!("SELECT {SUB_COLUMNS} FROM {SUB_FROM} WHERE s.id = $1");
    let row = sqlx::query_as::<_, SubRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching subscription")?;
    Ok(row.map(row_to_sub))
}

fn today() -> Date {
    OffsetDateTime::now_utc().date()
}

pub async fn add_subscription(pool: &PgPool, input: &SubscriptionInput) -> anyhow::Result<Subscription> {
    let id = Uuid::new_v4();
    let started = input.started_date();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO subscriptions \
            (id, name, platform, url, price, currency, frequency, category, started_on, active, \
             remind_days, remind_channels, trial_end, ended_at, yearly_price, payment_method, \
             card_expiry, my_share_pct) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
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
    .bind(input.remind_days)
    .bind(ids_json(&input.remind_channels))
    .bind(input.trial_date())
    .bind(input.ended_date())
    .bind(input.yearly_price)
    .bind(input.payment_method.as_deref())
    .bind(input.card_expiry.as_deref())
    .bind(input.my_share_pct)
    .execute(&mut *tx)
    .await
    .context("inserting subscription")?;
    // The history starts with the price the subscription was created with.
    sqlx::query(
        "INSERT INTO subscription_prices (subscription_id, price, currency, effective_from) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(input.price)
    .bind(&input.currency)
    .bind(started.unwrap_or_else(today))
    .execute(&mut *tx)
    .await
    .context("recording first subscription price")?;
    tx.commit().await?;
    get_subscription(pool, id)
        .await?
        .context("subscription vanished after insert")
}

/// Whether an edit changes what the subscription costs, so the history gets a row.
pub fn price_changed(old_price: f64, old_currency: &str, new_price: f64, new_currency: &str) -> bool {
    (old_price - new_price).abs() > 1e-9 || old_currency != new_currency
}

pub async fn update_subscription(
    pool: &PgPool,
    id: Uuid,
    input: &SubscriptionInput,
) -> anyhow::Result<bool> {
    let started = input.started_date();
    let mut tx = pool.begin().await?;
    let old: Option<(f64, String, Option<Date>)> = sqlx::query_as(
        "SELECT price, currency, started_on FROM subscriptions WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .context("loading subscription before update")?;
    let Some((old_price, old_currency, old_started)) = old else {
        return Ok(false);
    };
    // A reminder already sent stays sent unless the cadence it was computed from moved.
    sqlx::query(
        "UPDATE subscriptions SET \
            name = $2, platform = $3, url = $4, price = $5, currency = $6, frequency = $7, \
            category = $8, started_on = $9, active = $10, remind_days = $11, \
            remind_channels = $12, trial_end = $13, ended_at = $14, yearly_price = $15, \
            payment_method = $16, card_expiry = $17, my_share_pct = $18, \
            reminded_for = CASE WHEN frequency = $7 AND started_on IS NOT DISTINCT FROM $9 \
                                 AND trial_end IS NOT DISTINCT FROM $13 \
                                THEN reminded_for ELSE NULL END, \
            updated_at = now() \
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
    .bind(input.remind_days)
    .bind(ids_json(&input.remind_channels))
    .bind(input.trial_date())
    .bind(input.ended_date())
    .bind(input.yearly_price)
    .bind(input.payment_method.as_deref())
    .bind(input.card_expiry.as_deref())
    .bind(input.my_share_pct)
    .execute(&mut *tx)
    .await
    .context("updating subscription")?;
    if price_changed(old_price, &old_currency, input.price, &input.currency) {
        // A subscription created before the history existed (demo seed, an old row with no
        // backfill) first records the price it had, so the step starts from somewhere.
        sqlx::query(
            "INSERT INTO subscription_prices (subscription_id, price, currency, effective_from) \
             SELECT $1, $2, $3, $4 \
             WHERE NOT EXISTS (SELECT 1 FROM subscription_prices WHERE subscription_id = $1)",
        )
        .bind(id)
        .bind(old_price)
        .bind(&old_currency)
        .bind(old_started.unwrap_or_else(today))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO subscription_prices (subscription_id, price, currency, effective_from) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(input.price)
        .bind(&input.currency)
        .bind(today())
        .execute(&mut *tx)
        .await
        .context("appending subscription price")?;
    }
    tx.commit().await?;
    Ok(true)
}

pub async fn delete_subscription(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM subscriptions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Price history ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PricePoint {
    pub price: f64,
    pub currency: String,
    #[serde(serialize_with = "ser_date")]
    pub effective_from: Date,
}

fn ser_date<S: serde::Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&iso_date(*d))
}

/// A subscription's prices, oldest first. A subscription with no recorded history (created
/// before it existed and never repriced) reads as one row: its current price from its start.
pub async fn price_history(pool: &PgPool, sub: &Subscription) -> anyhow::Result<Vec<PricePoint>> {
    let rows: Vec<(f64, String, Date)> = sqlx::query_as(
        "SELECT price, currency, effective_from FROM subscription_prices \
         WHERE subscription_id = $1 ORDER BY effective_from, created_at",
    )
    .bind(sub.id)
    .fetch_all(pool)
    .await
    .context("loading subscription prices")?;
    if rows.is_empty() {
        return Ok(vec![PricePoint {
            price: sub.price,
            currency: sub.currency.clone(),
            effective_from: sub.started_on.unwrap_or_else(today),
        }]);
    }
    Ok(rows
        .into_iter()
        .map(|(price, currency, effective_from)| PricePoint { price, currency, effective_from })
        .collect())
}

async fn all_price_histories(pool: &PgPool) -> anyhow::Result<HashMap<Uuid, Vec<PricePoint>>> {
    let rows: Vec<(Uuid, f64, String, Date)> = sqlx::query_as(
        "SELECT subscription_id, price, currency, effective_from FROM subscription_prices \
         ORDER BY subscription_id, effective_from, created_at",
    )
    .fetch_all(pool)
    .await
    .context("loading subscription prices")?;
    let mut out: HashMap<Uuid, Vec<PricePoint>> = HashMap::new();
    for (id, price, currency, effective_from) in rows {
        out.entry(id).or_default().push(PricePoint { price, currency, effective_from });
    }
    Ok(out)
}

/// The price in force on `day`: the latest row from on or before it, else the first one.
pub fn price_at(history: &[PricePoint], day: Date) -> Option<&PricePoint> {
    history
        .iter()
        .rev()
        .find(|p| p.effective_from <= day)
        .or_else(|| history.first())
}

// ── Ratings ──────────────────────────────────────────────────────────────────

pub async fn add_rating(pool: &PgPool, id: Uuid, score: i16) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "INSERT INTO subscription_ratings (subscription_id, score) \
         SELECT id, $2 FROM subscriptions WHERE id = $1",
    )
    .bind(id)
    .bind(score)
    .execute(pool)
    .await
    .context("rating subscription")?;
    Ok(res.rows_affected() > 0)
}

// ── Autocomplete suggestions ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SubSuggestions {
    pub platforms: Vec<String>,
    pub categories: Vec<String>,
    pub payment_methods: Vec<String>,
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
        payment_methods: distinct(pool, "payment_method").await?,
    })
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Settings {
    pub display_currency: String,
    /// Monthly cap per category, in the display currency.
    pub category_budgets: BTreeMap<String, f64>,
    /// Channel ids for a budget breach; empty = every channel granted to Subscriptions.
    pub budget_channels: Vec<Uuid>,
    /// Category to the month (`YYYY-MM`) its breach was last notified.
    #[serde(skip)]
    pub budget_notified: BTreeMap<String, String>,
}

fn number_map(v: serde_json::Value) -> BTreeMap<String, f64> {
    v.as_object()
        .map(|o| o.iter().filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default()
}

fn string_map(v: serde_json::Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    let row: Option<(String, serde_json::Value, serde_json::Value, serde_json::Value)> =
        sqlx::query_as(
            "SELECT display_currency, category_budgets, budget_channels, budget_notified \
             FROM subscription_settings WHERE id = TRUE",
        )
        .fetch_optional(pool)
        .await
        .context("loading subscription settings")?;
    Ok(match row {
        Some((display_currency, budgets, channels, notified)) => Settings {
            display_currency,
            category_budgets: number_map(budgets),
            budget_channels: channel_ids(&channels),
            budget_notified: string_map(notified),
        },
        None => Settings {
            display_currency: "USD".to_string(),
            category_budgets: BTreeMap::new(),
            budget_channels: Vec::new(),
            budget_notified: BTreeMap::new(),
        },
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

pub async fn set_budgets(
    pool: &PgPool,
    budgets: Option<&BTreeMap<String, f64>>,
    channels: Option<&[Uuid]>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO subscription_settings (id) VALUES (TRUE) ON CONFLICT (id) DO NOTHING",
    )
    .execute(pool)
    .await?;
    if let Some(b) = budgets {
        sqlx::query(
            "UPDATE subscription_settings SET category_budgets = $1, updated_at = now() WHERE id = TRUE",
        )
        .bind(serde_json::to_value(b)?)
        .execute(pool)
        .await
        .context("saving category budgets")?;
    }
    if let Some(c) = channels {
        sqlx::query(
            "UPDATE subscription_settings SET budget_channels = $1, updated_at = now() WHERE id = TRUE",
        )
        .bind(ids_json(c))
        .execute(pool)
        .await
        .context("saving budget channels")?;
    }
    Ok(())
}

pub async fn set_budget_notified(pool: &PgPool, notified: &BTreeMap<String, String>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE subscription_settings SET budget_notified = $1 WHERE id = TRUE",
    )
    .bind(serde_json::to_value(notified)?)
    .execute(pool)
    .await
    .context("saving budget notified months")?;
    Ok(())
}

// ── Billing dates ────────────────────────────────────────────────────────────

/// `d` moved by `n` months, the day clamped to the target month's length (Jan 31 + 1 month
/// is Feb 28 or 29, never Mar 3).
pub fn add_months_clamped(d: Date, n: i32) -> Date {
    let total = d.year() * 12 + (d.month() as i32 - 1) + n;
    let year = total.div_euclid(12);
    let month = time::Month::try_from((total.rem_euclid(12) + 1) as u8).unwrap_or(time::Month::January);
    let last = month.length(year);
    Date::from_calendar_date(year, month, d.day().min(last)).unwrap_or(d)
}

/// The k-th billing date from `anchor`. Always computed from the anchor, so a short month
/// never drifts the following ones.
fn nth_billing(anchor: Date, frequency: &str, k: i32) -> Date {
    match frequency {
        "weekly" => anchor + Duration::days(7 * k as i64),
        "quarterly" => add_months_clamped(anchor, 3 * k),
        "yearly" => add_months_clamped(anchor, 12 * k),
        _ => add_months_clamped(anchor, k),
    }
}

/// Every billing date from `anchor` on its cadence that falls in `from..=to`.
pub fn billing_dates(anchor: Date, frequency: &str, from: Date, to: Date) -> Vec<Date> {
    let mut out = Vec::new();
    if to < from || to < anchor {
        return out;
    }
    // Skip whole periods before `from` instead of walking from an old anchor.
    let mut k = if from > anchor {
        let rough = match frequency {
            "weekly" => ((from - anchor).whole_days() / 7) as i32,
            "quarterly" => months_between(anchor, from) / 3,
            "yearly" => months_between(anchor, from) / 12,
            _ => months_between(anchor, from),
        };
        (rough - 1).max(0)
    } else {
        0
    };
    loop {
        let d = nth_billing(anchor, frequency, k);
        if d > to {
            break;
        }
        if d >= from {
            out.push(d);
        }
        k += 1;
    }
    out
}

/// A subscription's billing dates in `from..=to`: none while paused, none from its end date,
/// none before its trial ends. With no anchor, `fallback` stands in when given.
pub fn sub_billing_dates(s: &Subscription, from: Date, to: Date, fallback: Option<Date>) -> Vec<Date> {
    if !s.active {
        return Vec::new();
    }
    let Some(anchor) = s.anchor().or(fallback) else {
        return Vec::new();
    };
    let mut dates = billing_dates(anchor, &s.frequency, from, to);
    if let Some(end) = s.ended_at {
        dates.retain(|d| *d < end);
    }
    dates
}

/// The next billing date on or after `today`, if the subscription has one within a year.
pub fn next_billing(s: &Subscription, today: Date) -> Option<Date> {
    sub_billing_dates(s, today, today + Duration::days(400), None).into_iter().next()
}

/// Whether a renewal reminder is due today for the billing date `next`: the reminder window
/// has opened and this billing date was not reminded yet.
pub fn reminder_due(next: Date, remind_days: i32, reminded_for: Option<Date>, today: Date) -> bool {
    if reminded_for == Some(next) || today > next {
        return false;
    }
    next - Duration::days(remind_days.max(0) as i64) <= today
}

/// Subscriptions whose renewal reminder is due today, with the billing date it is for.
pub async fn due_reminders(pool: &PgPool, today: Date) -> anyhow::Result<Vec<(Subscription, Date)>> {
    let subs = list_subscriptions(pool, &SubFilter { active_only: true, ..Default::default() }).await?;
    Ok(subs
        .into_iter()
        .filter_map(|s| {
            let days = s.remind_days?;
            let next = next_billing(&s, today)?;
            reminder_due(next, days, s.reminded_for, today).then_some((s, next))
        })
        .collect())
}

pub async fn mark_reminded(pool: &PgPool, id: Uuid, billing: Date) -> anyhow::Result<()> {
    sqlx::query("UPDATE subscriptions SET reminded_for = $2 WHERE id = $1")
        .bind(id)
        .bind(billing)
        .execute(pool)
        .await
        .context("marking subscription reminded")?;
    Ok(())
}

/// Insert the in-app notification for a subscription event (reminder, budget breach).
pub async fn insert_notification(
    pool: &PgPool,
    linked_id: Option<Uuid>,
    name: &str,
    details: &str,
    url: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1, $2, 'subscription', $3, $4, $5, 'Subscriptions')",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(linked_id)
    .bind(details)
    .bind(url)
    .execute(pool)
    .await
    .context("inserting subscription notification")?;
    Ok(())
}

// ── Cancelled lifecycle ──────────────────────────────────────────────────────

/// Whole months from `ended` to `today` (a month counts once its day is reached).
pub fn whole_months_since(ended: Date, today: Date) -> i32 {
    if today <= ended {
        return 0;
    }
    let mut n = months_between(ended, today);
    if today.day() < ended.day() {
        n -= 1;
    }
    n.max(0)
}

/// What a cancelled subscription has saved so far: its monthly equivalent for every whole
/// month since it ended.
pub fn saved_since(monthly_equivalent: f64, ended: Date, today: Date) -> f64 {
    monthly_equivalent * whole_months_since(ended, today) as f64
}

// ── Budgets ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetBreach {
    pub category: String,
    pub amount: f64,
    pub cap: f64,
}

/// Categories over their cap that were not notified yet in `month` (`YYYY-MM`).
pub fn budget_breaches(
    totals: &[CategoryTotal],
    budgets: &BTreeMap<String, f64>,
    notified: &BTreeMap<String, String>,
    month: &str,
) -> Vec<BudgetBreach> {
    totals
        .iter()
        .filter_map(|t| {
            let cap = *budgets.get(&t.category)?;
            if cap <= 0.0 || t.amount <= cap + 1e-9 {
                return None;
            }
            if notified.get(&t.category).is_some_and(|m| m == month) {
                return None;
            }
            Some(BudgetBreach { category: t.category.clone(), amount: t.amount, cap })
        })
        .collect()
}

// ── Forecast ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ForecastMonth {
    /// `YYYY-MM`.
    pub month: String,
    /// Billed in the month, converted into `currency`.
    pub total: f64,
    pub currency: String,
    /// Billed in the month per billing currency, unconverted.
    pub by_currency: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize)]
pub struct Forecast {
    pub months: Vec<ForecastMonth>,
    pub monthly_avg: f64,
    pub currency: String,
    /// Subscriptions left out of the converted totals for lack of an FX rate.
    pub unconverted: i64,
}

/// Billing dates of `subs` grouped per calendar month over the 12 months after the one
/// containing `today`, each at the user's share. Returns `(month first day, sub index,
/// amount)` triples; a subscription with no start date bills from the current month's 1st.
pub fn forecast_lines(subs: &[Subscription], today: Date) -> Vec<(Date, usize, f64)> {
    let this_month = today.replace_day(1).unwrap_or(today);
    let from = add_months_clamped(this_month, 1);
    let to = add_months_clamped(this_month, 13) - Duration::days(1);
    let mut out = Vec::new();
    for (i, s) in subs.iter().enumerate() {
        for d in sub_billing_dates(s, from, to, Some(this_month)) {
            let m = d.replace_day(1).unwrap_or(d);
            out.push((m, i, s.price * s.share()));
        }
    }
    out
}

/// The next 12 calendar months of outflows (from next month), converted into
/// `display_currency` at today's rate.
pub async fn forecast(pool: &PgPool, display_currency: &str) -> anyhow::Result<Forecast> {
    let subs = list_subscriptions(pool, &SubFilter { active_only: true, ..Default::default() }).await?;
    let today = today();
    let this_month = today.replace_day(1).unwrap_or(today);
    let mut fx = FxCache::new();
    let mut months: Vec<ForecastMonth> = (1..=12)
        .map(|k| {
            let m = add_months_clamped(this_month, k);
            ForecastMonth {
                month: format!("{:04}-{:02}", m.year(), m.month() as u8),
                total: 0.0,
                currency: display_currency.to_string(),
                by_currency: BTreeMap::new(),
            }
        })
        .collect();
    let mut unconverted = std::collections::BTreeSet::new();
    for (m, i, amount) in forecast_lines(&subs, today) {
        let idx = months_between(this_month, m) - 1;
        let Some(slot) = usize::try_from(idx).ok().and_then(|i| months.get_mut(i)) else {
            continue;
        };
        let s = &subs[i];
        *slot.by_currency.entry(s.currency.clone()).or_insert(0.0) += amount;
        match fx.convert(pool, amount, &s.currency, display_currency, today).await? {
            Some(v) => slot.total += v,
            None => {
                unconverted.insert(s.id);
            }
        }
    }
    let monthly_avg = months.iter().map(|m| m.total).sum::<f64>() / 12.0;
    Ok(Forecast {
        months,
        monthly_avg,
        currency: display_currency.to_string(),
        unconverted: unconverted.len() as i64,
    })
}

// ── iCal export ──────────────────────────────────────────────────────────────

/// Escape a text value for an iCal property (RFC 5545 section 3.3.11).
pub fn ical_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Fold a content line at 75 octets, continuation lines starting with one space.
pub fn ical_fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for c in line.chars() {
        let w = c.len_utf8();
        if len + w > 75 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += w;
    }
    out
}

fn ical_day(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month() as u8, d.day())
}

/// One all-day event per billing date over the next 12 months. Subscriptions without a
/// start date have no billing day and are left out.
pub fn ical_calendar(subs: &[Subscription], today: Date, stamp: &str) -> String {
    let to = add_months_clamped(today, 12);
    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//OpenTraderWorld//Subscriptions//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "X-WR-CALNAME:Subscriptions".into(),
    ];
    for s in subs {
        for d in sub_billing_dates(s, today, to, None) {
            let amount = s.price * s.share();
            let summary = format!("{} {:.2} {}", s.name, amount, s.currency);
            lines.push("BEGIN:VEVENT".into());
            lines.push(format!("UID:{}-{}@opentraderworld", s.id, ical_day(d)));
            lines.push(format!("DTSTAMP:{stamp}"));
            lines.push(format!("DTSTART;VALUE=DATE:{}", ical_day(d)));
            lines.push(format!("DTEND;VALUE=DATE:{}", ical_day(d + Duration::days(1))));
            lines.push(format!("SUMMARY:{}", ical_escape(&summary)));
            if let Some(m) = s.payment_method.as_deref().filter(|m| !m.is_empty()) {
                lines.push(format!("DESCRIPTION:{}", ical_escape(m)));
            }
            lines.push("TRANSP:TRANSPARENT".into());
            lines.push("END:VEVENT".into());
        }
    }
    lines.push("END:VCALENDAR".into());
    let mut out = String::new();
    for l in lines {
        out.push_str(&ical_fold(&l));
        out.push_str("\r\n");
    }
    out
}

// ── Breakdown ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MonthPoint {
    /// First day of the month (YYYY-MM-01) as ISO date.
    pub month: String,
    pub amount: f64,
    /// Monthly-equivalent cost of every subscription running that month, at the price it had
    /// then: the "total cost creep" line.
    pub monthly_equiv: f64,
    /// Per-subscription contribution to this month's billed amount, keyed by sub id (string).
    /// Lets the chart stack one colored segment per subscription.
    pub by_sub: HashMap<String, f64>,
    /// Per-category contribution to this month's billed amount, keyed by category name.
    pub by_category: HashMap<String, f64>,
}

/// A subscription's identity for the chart legend / stacking order.
#[derive(Debug, Serialize)]
pub struct SubLegend {
    pub id: String,
    pub name: String,
}

/// Monthly-equivalent recurring spend grouped by category (converted to display currency).
#[derive(Debug, Clone, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub amount: f64,
}

#[derive(Debug, Serialize)]
pub struct Breakdown {
    pub display_currency: String,
    /// Monthly-equivalent recurring spend (all running subs, at the user's share), converted.
    pub monthly_total: f64,
    /// Yearly-equivalent (monthly_total × 12).
    pub yearly_total: f64,
    /// Amount actually billed next calendar month (frequency + start-date aware).
    pub next_month_total: f64,
    /// Per-month columns for the chart.
    pub months: Vec<MonthPoint>,
    /// Per-calendar-year columns: every running sub's FULL annual cost (yearly-equivalent),
    /// counted whole for any year it's running, so future years aren't understated by a
    /// window that doesn't span the whole year. `month` holds `YYYY-01-01`.
    pub years: Vec<MonthPoint>,
    /// Subscriptions contributing to the breakdown (stable legend / stacking order).
    pub subs: Vec<SubLegend>,
    /// Monthly-equivalent spend per category, ranked high to low.
    pub categories: Vec<CategoryTotal>,
    pub active_count: i64,
    /// Cancelled subscriptions (end date reached).
    pub ended_count: i64,
    /// What the cancelled subscriptions would have cost since they ended.
    pub saved_since_cancelled: f64,
    /// Subs whose currency could not be converted (no FX rate), excluded from totals.
    pub unconverted: i64,
}

fn category_of(s: &Subscription) -> String {
    s.category
        .as_deref()
        .filter(|c| !c.is_empty())
        .unwrap_or("Uncategorized")
        .to_string()
}

/// Whether a subscription bills in the calendar month containing `target` (first-of-month),
/// given its frequency and anchor. Subs with no anchor are treated as billing every
/// applicable period anchored to the current month.
fn bills_in_month(sub: &Subscription, target: Date, anchor: Date) -> bool {
    let start = sub.anchor().unwrap_or(anchor);
    let start_month = start.replace_day(1).unwrap_or(start);
    if start_month > target || !running_in_month(sub, target) {
        return false;
    }
    let n = months_between(start, target);
    match sub.frequency.as_str() {
        "quarterly" => n % 3 == 0,
        "yearly" => n % 12 == 0,
        // Weekly and monthly bill every month.
        _ => true,
    }
}

/// Not yet ended in the month starting `m` (a sub ending mid-month still counts there).
fn running_in_month(sub: &Subscription, m: Date) -> bool {
    sub.ended_at.is_none_or(|e| m < e)
}

/// Monthly totals per category over the subscriptions running today, converted into
/// `display_currency`, ranked high to low. Also returns the number of unconverted subs.
async fn category_totals(
    pool: &PgPool,
    fx: &mut FxCache,
    subs: &[&Subscription],
    display_currency: &str,
    today: Date,
) -> anyhow::Result<(Vec<CategoryTotal>, f64, i64)> {
    let mut by_category: HashMap<String, f64> = HashMap::new();
    let mut total = 0.0;
    let mut unconverted = 0i64;
    for s in subs {
        match fx.convert(pool, s.monthly_equivalent(), &s.currency, display_currency, today).await? {
            Some(v) => {
                total += v;
                *by_category.entry(category_of(s)).or_insert(0.0) += v;
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
    Ok((categories, total, unconverted))
}

/// Monthly-equivalent spend per category for the subscriptions running today, for the
/// budget check.
pub async fn current_category_totals(
    pool: &PgPool,
    display_currency: &str,
) -> anyhow::Result<Vec<CategoryTotal>> {
    let subs = list_subscriptions(pool, &SubFilter { active_only: true, ..Default::default() }).await?;
    let today = today();
    let live: Vec<&Subscription> = subs.iter().filter(|s| !s.ended_by(today)).collect();
    let mut fx = FxCache::new();
    Ok(category_totals(pool, &mut fx, &live, display_currency, today).await?.0)
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
    // Only active (not paused) subs contribute to recurring spend.
    let f = SubFilter {
        platform: filter.platform.clone(),
        category: filter.category.clone(),
        active_only: true,
    };
    let subs = list_subscriptions(pool, &f).await?;
    let histories = all_price_histories(pool).await?;

    let today = today();
    let this_month = today.replace_day(1).unwrap_or(today);
    let mut fx = FxCache::new();

    // Running today: counted in the headline totals. Cancelled ones only feed the past
    // months of the chart and the "saved since cancelled" counter.
    let live: Vec<&Subscription> = subs.iter().filter(|s| !s.ended_by(today)).collect();
    let (categories, monthly_total, unconverted) =
        category_totals(pool, &mut fx, &live, display_currency, today).await?;

    let mut saved_since_cancelled = 0.0;
    let mut ended_count = 0i64;
    for s in subs.iter().filter(|s| s.ended_by(today)) {
        ended_count += 1;
        let Some(end) = s.ended_at else { continue };
        let saved = saved_since(s.monthly_equivalent(), end, today);
        if let Some(v) = fx.convert(pool, saved, &s.currency, display_currency, today).await? {
            saved_since_cancelled += v;
        }
    }

    // The price a sub had in the month starting `m` (its history, else its current price).
    let price_in = |s: &Subscription, m: Date| -> (f64, String) {
        let month_end = add_months_clamped(m, 1) - Duration::days(1);
        histories
            .get(&s.id)
            .and_then(|h| price_at(h, month_end.min(today)))
            .map(|p| (p.price, p.currency.clone()))
            .unwrap_or_else(|| (s.price, s.currency.clone()))
    };

    // Per-month columns: the amount actually billed in each calendar month (frequency +
    // start-date aware), so yearly/quarterly subs spike in their billing month rather than
    // being smoothed flat.
    let mut months = Vec::new();
    for off in -months_back..=months_fwd {
        let m = add_months_clamped(this_month, off as i32);
        let mut amount = 0.0;
        let mut monthly_equiv = 0.0;
        let mut by_sub = HashMap::new();
        // `by_category` uses the monthly-EQUIVALENT for every sub running that month, so
        // category bars are smoothed and comparable rather than spiking in a billing month
        // (unlike `amount`/`by_sub`, which are billed).
        let mut by_category: HashMap<String, f64> = HashMap::new();
        for s in &subs {
            let (price, currency) = price_in(s, m);
            let mine = price * s.share();
            if bills_in_month(s, m, this_month) {
                if let Some(v) = fx.convert(pool, mine, &currency, display_currency, today).await? {
                    amount += v;
                    by_sub.insert(s.id.to_string(), v);
                }
            }
            let started = s.anchor().is_none_or(|d| d.replace_day(1).unwrap_or(d) <= m);
            if started && running_in_month(s, m) {
                let monthly = mine * monthly_factor(&s.frequency);
                if let Some(v) = fx.convert(pool, monthly, &currency, display_currency, today).await? {
                    monthly_equiv += v;
                    *by_category.entry(category_of(s)).or_insert(0.0) += v;
                }
            }
        }
        months.push(MonthPoint {
            month: format_month(m),
            amount,
            monthly_equiv,
            by_sub,
            by_category,
        });
    }

    // Per-calendar-year columns. Each running sub contributes its FULL annual cost
    // (yearly-equivalent = monthly-equivalent × 12) to every year it runs: start and end
    // aware, but never prorated, so a future year shows the complete spend.
    let first_year = add_months_clamped(this_month, -months_back as i32).year();
    let last_year = add_months_clamped(this_month, months_fwd as i32).year();
    let mut years = Vec::new();
    for y in first_year..=last_year {
        let year_start = Date::from_calendar_date(y, time::Month::January, 1).unwrap_or(this_month);
        let year_end = Date::from_calendar_date(y, time::Month::December, 31).unwrap_or(this_month);
        let mut amount = 0.0;
        let mut by_sub = HashMap::new();
        let mut by_category: HashMap<String, f64> = HashMap::new();
        for s in &subs {
            let started = s.anchor().is_none_or(|d| d <= year_end);
            let not_ended = s.ended_at.is_none_or(|e| e > year_start);
            if started && not_ended {
                let yearly = s.monthly_equivalent() * 12.0;
                if let Some(v) = fx.convert(pool, yearly, &s.currency, display_currency, today).await? {
                    amount += v;
                    by_sub.insert(s.id.to_string(), v);
                    *by_category.entry(category_of(s)).or_insert(0.0) += v;
                }
            }
        }
        years.push(MonthPoint {
            month: format!("{y:04}-01-01"),
            amount,
            monthly_equiv: amount / 12.0,
            by_sub,
            by_category,
        });
    }

    // Next calendar month's actual billed amount (frequency + start aware), converted.
    let next_month = add_months_clamped(this_month, 1);
    let mut next_month_total = 0.0;
    for s in &live {
        if bills_in_month(s, next_month, this_month) {
            if let Some(v) =
                fx.convert(pool, s.price * s.share(), &s.currency, display_currency, today).await?
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
        active_count: live.len() as i64,
        ended_count,
        saved_since_cancelled,
        unconverted,
    })
}

fn format_month(d: Date) -> String {
    format!("{:04}-{:02}-01", d.year(), d.month() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn sub(frequency: &str, started: Option<Date>) -> Subscription {
        Subscription {
            id: Uuid::nil(),
            name: "Test".into(),
            platform: None,
            url: None,
            price: 10.0,
            currency: "USD".into(),
            frequency: frequency.into(),
            category: None,
            started_on: started,
            active: true,
            remind_days: None,
            remind_channels: Vec::new(),
            reminded_for: None,
            trial_end: None,
            ended_at: None,
            yearly_price: None,
            payment_method: None,
            card_expiry: None,
            my_share_pct: 100.0,
            last_score: None,
            last_rated_at: None,
        }
    }

    #[test]
    fn subscriptions_price_changes_append_and_same_price_does_not() {
        assert!(price_changed(10.0, "USD", 12.0, "USD"));
        assert!(price_changed(10.0, "USD", 10.0, "EUR"));
        assert!(!price_changed(10.0, "USD", 10.0, "USD"));
    }

    #[test]
    fn subscriptions_price_at_picks_the_row_in_force() {
        let h = vec![
            PricePoint { price: 10.0, currency: "USD".into(), effective_from: date!(2026 - 01 - 01) },
            PricePoint { price: 12.0, currency: "USD".into(), effective_from: date!(2026 - 06 - 15) },
        ];
        assert_eq!(price_at(&h, date!(2026 - 06 - 14)).unwrap().price, 10.0);
        assert_eq!(price_at(&h, date!(2026 - 06 - 15)).unwrap().price, 12.0);
        // Before the first row: the first price, never nothing.
        assert_eq!(price_at(&h, date!(2025 - 01 - 01)).unwrap().price, 10.0);
    }

    #[test]
    fn subscriptions_month_end_clamps_without_drift() {
        let a = date!(2026 - 01 - 31);
        assert_eq!(add_months_clamped(a, 1), date!(2026 - 02 - 28));
        assert_eq!(add_months_clamped(a, 2), date!(2026 - 03 - 31));
        assert_eq!(add_months_clamped(date!(2024 - 02 - 29), 12), date!(2025 - 02 - 28));
        assert_eq!(add_months_clamped(date!(2026 - 01 - 15), -1), date!(2025 - 12 - 15));
    }

    #[test]
    fn subscriptions_expansion_for_each_frequency() {
        let from = date!(2026 - 01 - 01);
        let to = date!(2026 - 12 - 31);
        let a = date!(2025 - 11 - 10);
        assert_eq!(billing_dates(a, "monthly", from, to).len(), 12);
        assert_eq!(billing_dates(a, "monthly", from, to)[0], date!(2026 - 01 - 10));
        let q = billing_dates(a, "quarterly", from, to);
        assert_eq!(q, vec![date!(2026 - 02 - 10), date!(2026 - 05 - 10), date!(2026 - 08 - 10), date!(2026 - 11 - 10)]);
        assert_eq!(billing_dates(a, "yearly", from, to), vec![date!(2026 - 11 - 10)]);
        let w = billing_dates(date!(2026 - 01 - 01), "weekly", from, to);
        assert_eq!(w.len(), 53);
        assert!(w.windows(2).all(|p| (p[1] - p[0]).whole_days() == 7));
        // An anchor in the future bills from itself only.
        assert_eq!(billing_dates(date!(2026 - 12 - 01), "monthly", from, to), vec![date!(2026 - 12 - 01)]);
    }

    #[test]
    fn subscriptions_trial_end_paused_and_ended_shape_billing() {
        let from = date!(2026 - 01 - 01);
        let to = date!(2026 - 06 - 30);
        let mut s = sub("monthly", Some(date!(2025 - 12 - 05)));
        s.trial_end = Some(date!(2026 - 03 - 20));
        assert_eq!(sub_billing_dates(&s, from, to, None)[0], date!(2026 - 03 - 20));
        s.trial_end = None;
        s.ended_at = Some(date!(2026 - 03 - 05));
        assert_eq!(sub_billing_dates(&s, from, to, None), vec![date!(2026 - 01 - 05), date!(2026 - 02 - 05)]);
        s.active = false;
        assert!(sub_billing_dates(&s, from, to, None).is_empty());
        // No start date: no billing day unless the caller supplies one.
        let n = sub("monthly", None);
        assert!(sub_billing_dates(&n, from, to, None).is_empty());
        assert_eq!(sub_billing_dates(&n, from, to, Some(from)).len(), 6);
    }

    #[test]
    fn subscriptions_reminder_fires_once_per_billing_date() {
        let next = date!(2026 - 03 - 10);
        assert!(!reminder_due(next, 3, None, date!(2026 - 03 - 06)));
        assert!(reminder_due(next, 3, None, date!(2026 - 03 - 07)));
        assert!(reminder_due(next, 3, None, date!(2026 - 03 - 10)));
        // Already reminded for this date: silent until the next billing date.
        assert!(!reminder_due(next, 3, Some(next), date!(2026 - 03 - 08)));
        assert!(reminder_due(date!(2026 - 04 - 10), 3, Some(next), date!(2026 - 04 - 08)));
        // Zero days: on the day itself.
        assert!(reminder_due(next, 0, None, next));
        assert!(!reminder_due(next, 0, None, date!(2026 - 03 - 09)));
    }

    #[test]
    fn subscriptions_saved_since_cancelled_counts_whole_months() {
        let end = date!(2026 - 01 - 15);
        assert_eq!(whole_months_since(end, date!(2026 - 01 - 20)), 0);
        assert_eq!(whole_months_since(end, date!(2026 - 02 - 14)), 0);
        assert_eq!(whole_months_since(end, date!(2026 - 02 - 15)), 1);
        assert_eq!(whole_months_since(end, date!(2026 - 07 - 16)), 6);
        assert_eq!(whole_months_since(end, date!(2025 - 12 - 01)), 0);
        assert_eq!(saved_since(12.5, end, date!(2026 - 04 - 15)), 37.5);
    }

    #[test]
    fn subscriptions_shared_plan_totals_use_the_share() {
        let mut s = sub("yearly", None);
        s.price = 120.0;
        s.my_share_pct = 50.0;
        assert!((s.monthly_equivalent() - 5.0).abs() < 1e-9);
        assert_eq!(share_factor(0.0), 1.0);
        assert_eq!(share_factor(250.0), 1.0);
        let lines = forecast_lines(&[s], date!(2026 - 01 - 10));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].2, 60.0);
    }

    #[test]
    fn subscriptions_forecast_covers_the_next_twelve_months() {
        let today = date!(2026 - 11 - 06);
        let m = sub("monthly", Some(date!(2026 - 01 - 31)));
        let q = sub("quarterly", Some(date!(2026 - 01 - 01)));
        let w = sub("weekly", Some(date!(2026 - 11 - 02)));
        let lines = forecast_lines(&[m, q, w], today);
        let count = |i: usize| lines.iter().filter(|l| l.1 == i).count();
        assert_eq!(count(0), 12);
        assert_eq!(count(1), 4);
        assert!((51..=53).contains(&count(2)));
        // Nothing in the current month, nothing past the twelfth.
        assert!(lines.iter().all(|l| l.0 >= date!(2026 - 12 - 01) && l.0 <= date!(2027 - 11 - 01)));
        // A no-anchor sub bills monthly from the current month's 1st.
        let n = sub("monthly", None);
        assert_eq!(forecast_lines(&[n], today).len(), 12);
    }

    #[test]
    fn subscriptions_budget_breach_notifies_once_a_month() {
        let totals = vec![
            CategoryTotal { category: "Data".into(), amount: 120.0 },
            CategoryTotal { category: "News".into(), amount: 20.0 },
        ];
        let budgets: BTreeMap<String, f64> = [("Data".to_string(), 100.0), ("News".to_string(), 50.0)].into();
        let mut notified = BTreeMap::new();
        let b = budget_breaches(&totals, &budgets, &notified, "2026-11");
        assert_eq!(b, vec![BudgetBreach { category: "Data".into(), amount: 120.0, cap: 100.0 }]);
        notified.insert("Data".to_string(), "2026-11".to_string());
        assert!(budget_breaches(&totals, &budgets, &notified, "2026-11").is_empty());
        assert_eq!(budget_breaches(&totals, &budgets, &notified, "2026-12").len(), 1);
    }

    #[test]
    fn subscriptions_ical_lines_are_escaped_folded_and_expanded() {
        assert_eq!(ical_escape("a,b;c\\d\ne"), "a\\,b\\;c\\\\d\\ne");
        let long = "SUMMARY:".to_string() + &"x".repeat(100);
        let folded = ical_fold(&long);
        let parts: Vec<&str> = folded.split("\r\n").collect();
        assert_eq!(parts[0].len(), 75);
        assert!(parts[1].starts_with(' '));
        assert_eq!(parts.concat().replacen(' ', "", 1), long);

        let mut s = sub("monthly", Some(date!(2026 - 01 - 31)));
        s.name = "Data, feed".into();
        let cal = ical_calendar(&[s], date!(2026 - 11 - 06), "20261106T000000Z");
        assert!(cal.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(cal.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(cal.matches("BEGIN:VEVENT").count(), 12);
        assert!(cal.contains("DTSTART;VALUE=DATE:20261130\r\n"));
        assert!(cal.contains("DTSTART;VALUE=DATE:20270228\r\n"));
        assert!(cal.contains("SUMMARY:Data\\, feed 10.00 USD\r\n"));
        assert!(cal.lines().all(|l| l.len() <= 76));
    }

    #[test]
    fn subscriptions_card_expiry_format() {
        assert!(valid_card_expiry("2027-03"));
        assert!(!valid_card_expiry("2027-13"));
        assert!(!valid_card_expiry("27-03"));
        assert!(!valid_card_expiry("2027/03"));
    }
}
