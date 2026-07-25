//! Storage + analytics for the MyWealth module.
//!
//! Assets are described by templates (reserved price/quantity fields + custom fields, like
//! the journal). Each update inserts a revision; the net-worth breakdown sums each asset's
//! latest revision on/before each sampled date, converting into a display currency via the
//! shared `journal_fx` rates. Single-user.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::{format_description::well_known::Iso8601, Date, OffsetDateTime};
use uuid::Uuid;

pub const ASSET_TYPES: [&str; 7] =
    ["money", "stock", "crypto", "watch", "house", "vehicle", "other"];

mod date_iso {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub asset_type: String,
    pub fields: JsonValue,
    pub is_builtin: bool,
    pub position: f64,
}

pub async fn list_templates(pool: &PgPool) -> anyhow::Result<Vec<Template>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, String, JsonValue, bool, f64)>(
        "SELECT id, name, description, asset_type, fields, is_builtin, position \
         FROM wealth_templates ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing wealth templates")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, description, asset_type, fields, is_builtin, position)| Template {
            id,
            name,
            description,
            asset_type,
            fields,
            is_builtin,
            position,
        })
        .collect())
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TemplateInput {
    #[serde(default)]
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "default_type")]
    pub asset_type: String,
    #[serde(default = "empty_array")]
    pub fields: JsonValue,
}

fn default_type() -> String {
    "other".to_string()
}
fn empty_array() -> JsonValue {
    JsonValue::Array(vec![])
}

pub async fn add_template(pool: &PgPool, input: &TemplateInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM wealth_templates")
        .fetch_one(pool)
        .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO wealth_templates (id, name, description, asset_type, fields, position) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.description.as_deref())
    .bind(&input.asset_type)
    .bind(&input.fields)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting wealth template")?;
    Ok(id)
}

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct TemplatePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub asset_type: Option<String>,
    pub fields: Option<JsonValue>,
    pub position: Option<f64>,
}

pub async fn update_template(pool: &PgPool, id: Uuid, patch: &TemplatePatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE wealth_templates SET \
            name = COALESCE($2, name), \
            description = COALESCE($3, description), \
            asset_type = COALESCE($4, asset_type), \
            fields = COALESCE($5, fields), \
            position = COALESCE($6, position), \
            updated_at = now() \
         WHERE id = $1 AND NOT is_builtin",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.description.as_deref())
    .bind(patch.asset_type.as_deref())
    .bind(patch.fields.as_ref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating wealth template")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_template(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM wealth_templates WHERE id = $1 AND NOT is_builtin")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Assets ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Asset {
    pub id: Uuid,
    pub template_id: Option<Uuid>,
    pub name: String,
    pub asset_type: String,
    pub currency: String,
    pub category: Option<String>,
    /// Latest revision's value in the asset's currency, or None if never valued.
    pub latest_value: Option<f64>,
    #[serde(with = "date_opt")]
    pub latest_at: Option<Date>,
    pub revision_count: i64,
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
pub struct AssetInput {
    pub template_id: Option<Uuid>,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_type")]
    pub asset_type: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    pub category: Option<String>,
}

fn default_currency() -> String {
    "USD".to_string()
}

#[derive(Debug, Default, Deserialize)]
pub struct AssetFilter {
    pub asset_type: Option<String>,
    pub category: Option<String>,
}

pub async fn list_assets(pool: &PgPool, filter: &AssetFilter) -> anyhow::Result<Vec<Asset>> {
    // Latest revision per asset via a lateral join.
    let sql = "SELECT a.id, a.template_id, a.name, a.asset_type, a.currency, a.category, \
                      r.value, r.valued_at, \
                      (SELECT COUNT(*) FROM wealth_revisions x WHERE x.asset_id = a.id) AS rc \
               FROM wealth_assets a \
               LEFT JOIN LATERAL ( \
                   SELECT value, valued_at FROM wealth_revisions \
                   WHERE asset_id = a.id ORDER BY valued_at DESC, created_at DESC LIMIT 1 \
               ) r ON TRUE \
               WHERE ($1::text IS NULL OR a.asset_type = $1) \
                 AND ($2::text IS NULL OR a.category = $2) \
               ORDER BY a.name, a.created_at";
    let rows = sqlx::query_as::<
        _,
        (Uuid, Option<Uuid>, String, String, String, Option<String>, Option<f64>, Option<Date>, i64),
    >(sql)
    .bind(filter.asset_type.as_deref().filter(|s| !s.is_empty()))
    .bind(filter.category.as_deref().filter(|s| !s.is_empty()))
    .fetch_all(pool)
    .await
    .context("listing assets")?;
    Ok(rows
        .into_iter()
        .map(
            |(id, template_id, name, asset_type, currency, category, latest_value, latest_at, rc)| Asset {
                id,
                template_id,
                name,
                asset_type,
                currency,
                category,
                latest_value,
                latest_at,
                revision_count: rc,
            },
        )
        .collect())
}

pub async fn add_asset(pool: &PgPool, input: &AssetInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wealth_assets (id, template_id, name, asset_type, currency, category) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(input.template_id)
    .bind(&input.name)
    .bind(&input.asset_type)
    .bind(&input.currency)
    .bind(input.category.as_deref())
    .execute(pool)
    .await
    .context("inserting asset")?;
    Ok(id)
}

pub async fn update_asset(pool: &PgPool, id: Uuid, input: &AssetInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE wealth_assets SET \
            template_id = $2, name = $3, asset_type = $4, currency = $5, category = $6, \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(input.template_id)
    .bind(&input.name)
    .bind(&input.asset_type)
    .bind(&input.currency)
    .bind(input.category.as_deref())
    .execute(pool)
    .await
    .context("updating asset")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_asset(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM wealth_assets WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Revisions ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Revision {
    pub id: Uuid,
    pub asset_id: Uuid,
    #[serde(with = "date_iso")]
    pub valued_at: Date,
    pub price: Option<f64>,
    pub quantity: Option<f64>,
    pub value: f64,
    pub fields: JsonValue,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RevisionInput {
    /// `YYYY-MM-DD`; defaults to today when omitted.
    pub valued_at: Option<String>,
    pub price: Option<f64>,
    pub quantity: Option<f64>,
    /// Explicit value; when omitted and price+quantity are set, value = price×quantity.
    pub value: Option<f64>,
    #[serde(default = "empty_object")]
    pub fields: JsonValue,
    pub note: Option<String>,
}

fn empty_object() -> JsonValue {
    JsonValue::Object(Default::default())
}

impl RevisionInput {
    fn date(&self) -> Date {
        self.valued_at
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| Date::parse(s, &Iso8601::DATE).ok())
            .unwrap_or_else(|| OffsetDateTime::now_utc().date())
    }
    /// Resolved value: explicit, else price×quantity, else 0.
    fn resolved_value(&self) -> f64 {
        if let Some(v) = self.value {
            return v;
        }
        match (self.price, self.quantity) {
            (Some(p), Some(q)) => p * q,
            (Some(p), None) => p,
            _ => 0.0,
        }
    }
}

pub async fn list_revisions(pool: &PgPool, asset_id: Uuid) -> anyhow::Result<Vec<Revision>> {
    let rows = sqlx::query_as::<
        _,
        (Uuid, Uuid, Date, Option<f64>, Option<f64>, f64, JsonValue, Option<String>),
    >(
        "SELECT id, asset_id, valued_at, price, quantity, value, fields, note \
         FROM wealth_revisions WHERE asset_id = $1 ORDER BY valued_at DESC, created_at DESC",
    )
    .bind(asset_id)
    .fetch_all(pool)
    .await
    .context("listing revisions")?;
    Ok(rows
        .into_iter()
        .map(|(id, asset_id, valued_at, price, quantity, value, fields, note)| Revision {
            id,
            asset_id,
            valued_at,
            price,
            quantity,
            value,
            fields,
            note,
        })
        .collect())
}

pub async fn add_revision(pool: &PgPool, asset_id: Uuid, input: &RevisionInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wealth_revisions (id, asset_id, valued_at, price, quantity, value, fields, note) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(asset_id)
    .bind(input.date())
    .bind(input.price)
    .bind(input.quantity)
    .bind(input.resolved_value())
    .bind(&input.fields)
    .bind(input.note.as_deref())
    .execute(pool)
    .await
    .context("inserting revision")?;
    Ok(id)
}

pub async fn update_revision(pool: &PgPool, id: Uuid, input: &RevisionInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE wealth_revisions \
         SET valued_at = $2, price = $3, quantity = $4, value = $5, fields = $6, note = $7 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(input.date())
    .bind(input.price)
    .bind(input.quantity)
    .bind(input.resolved_value())
    .bind(&input.fields)
    .bind(input.note.as_deref())
    .execute(pool)
    .await
    .context("updating revision")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_revision(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM wealth_revisions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Settings {
    pub display_currency: String,
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT display_currency FROM wealth_settings WHERE id = TRUE")
            .fetch_optional(pool)
            .await
            .context("loading wealth settings")?;
    Ok(Settings {
        display_currency: row.map(|r| r.0).unwrap_or_else(|| "USD".to_string()),
    })
}

pub async fn set_display_currency(pool: &PgPool, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO wealth_settings (id, display_currency, updated_at) \
         VALUES (TRUE, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET display_currency = $1, updated_at = now()",
    )
    .bind(currency)
    .execute(pool)
    .await
    .context("setting wealth display currency")?;
    Ok(())
}

// ── Net-worth breakdown ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NetWorthPoint {
    #[serde(with = "date_iso")]
    pub at: Date,
    pub net_worth: f64,
}

#[derive(Debug, Serialize)]
pub struct Breakdown {
    pub display_currency: String,
    pub net_worth: f64,
    pub asset_count: i64,
    pub points: Vec<NetWorthPoint>,
    /// Net worth per category at the latest date, in the display currency (uncategorized
    /// assets keyed as ""). Powers the "by categories" bar view.
    pub by_category: std::collections::BTreeMap<String, f64>,
    /// Assets excluded from a point because no FX rate was available for their currency.
    pub unconverted: i64,
}

/// Each asset's latest revision value at or before a given date, in the asset's currency.
struct AssetValue {
    currency: String,
    category: String,
    /// (valued_at, value) revisions sorted ascending by date.
    revisions: Vec<(Date, f64)>,
}

impl AssetValue {
    fn value_on(&self, date: Date) -> Option<f64> {
        self.revisions
            .iter()
            .rev()
            .find(|(d, _)| *d <= date)
            .map(|(_, v)| *v)
    }
}

/// Net-worth time series: at each sampled period-end (and today) sum every asset's latest
/// revision on/before that date, converted to the display currency at that date's rate.
/// `granularity` is "month" (month-ends) or "year" (year-ends).
pub async fn breakdown(
    pool: &PgPool,
    filter: &AssetFilter,
    display_currency: &str,
    points_back: i64,
    granularity: &str,
) -> anyhow::Result<Breakdown> {
    // Load assets in scope and their revisions.
    let assets = list_assets(pool, filter).await?;
    let mut values: Vec<AssetValue> = Vec::new();
    for a in &assets {
        let revs = sqlx::query_as::<_, (Date, f64)>(
            "SELECT valued_at, value FROM wealth_revisions WHERE asset_id = $1 \
             ORDER BY valued_at ASC, created_at ASC",
        )
        .bind(a.id)
        .fetch_all(pool)
        .await
        .context("loading revisions for breakdown")?;
        values.push(AssetValue {
            currency: a.currency.clone(),
            category: a.category.clone().unwrap_or_default(),
            revisions: revs,
        });
    }

    let today = OffsetDateTime::now_utc().date();
    let yearly = granularity == "year";
    // Sample the last `points_back` period-ends (month or year) plus today.
    let mut dates: Vec<Date> = Vec::new();
    for off in (0..points_back).rev() {
        dates.push(if yearly {
            year_end_back(today, off)
        } else {
            month_end_back(today, off)
        });
    }
    if dates.last() != Some(&today) {
        dates.push(today);
    }

    let mut points = Vec::new();
    let mut unconverted = 0i64;
    let mut by_category: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for (idx, d) in dates.iter().enumerate() {
        let is_latest = idx == dates.len() - 1;
        let mut sum = 0.0;
        for (av, asset) in values.iter().zip(&assets) {
            if let Some(native) = av.value_on(*d) {
                match crate::journal_fx::convert(pool, native, &av.currency, display_currency, *d)
                    .await?
                {
                    Some(v) => {
                        sum += v;
                        if is_latest {
                            *by_category.entry(av.category.clone()).or_insert(0.0) += v;
                        }
                    }
                    // Only count unconverted once (on the latest point) to avoid inflation.
                    None => {
                        if is_latest {
                            unconverted += 1;
                        }
                        let _ = asset;
                    }
                }
            }
        }
        points.push(NetWorthPoint { at: *d, net_worth: sum });
    }

    let net_worth = points.last().map(|p| p.net_worth).unwrap_or(0.0);
    Ok(Breakdown {
        display_currency: display_currency.to_string(),
        net_worth,
        asset_count: assets.len() as i64,
        points,
        by_category,
        unconverted,
    })
}

/// Dec 31 of the year `n` years before `from`.
fn year_end_back(from: Date, n: i64) -> Date {
    let year = from.year() - n as i32;
    Date::from_calendar_date(year, time::Month::December, 31).unwrap_or(from)
}

/// The last day of the month `n` months before `from`.
fn month_end_back(from: Date, n: i64) -> Date {
    let total = (from.year() as i64) * 12 + (from.month() as i64 - 1) - n;
    let year = total.div_euclid(12) as i32;
    let month0 = total.rem_euclid(12) as u8;
    let month = time::Month::try_from(month0 + 1).unwrap_or(time::Month::January);
    // First of next month minus one day = last day of this month.
    let first = Date::from_calendar_date(year, month, 1).unwrap_or(from);
    let next = if month0 == 11 {
        Date::from_calendar_date(year + 1, time::Month::January, 1)
    } else {
        Date::from_calendar_date(year, time::Month::try_from(month0 + 2).unwrap(), 1)
    }
    .unwrap_or(first);
    next.previous_day().unwrap_or(first)
}
