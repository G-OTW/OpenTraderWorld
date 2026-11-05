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

/// What a wealth asset can be.
///
/// `stock` and `crypto` are kept so older rows still open, but they are no longer offered
/// when creating one: anything with a price series belongs to the Portfolio Tracker, and a
/// wealth asset points at a portfolio instead of copying its value. `loan` is the liability
/// side, which is what makes the headline an actual net worth.
pub const ASSET_TYPES: [&str; 9] = [
    "money", "portfolio", "loan", "watch", "house", "vehicle", "other", "stock", "crypto",
];

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
    /// +1 owned, -1 owed. What the net-worth sum multiplies this asset's value by.
    pub sign: i16,
    /// When set, the value is a portfolio's net worth read live, and the revisions below are
    /// history rather than the source of truth.
    pub portfolio_id: Option<Uuid>,
    /// Days after which the valuation is considered stale. 0 = never nag.
    pub review_days: i32,
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
    /// +1 owned, -1 owed. Anything else is refused rather than clamped.
    #[serde(default = "one")]
    pub sign: i16,
    /// Value this asset from a portfolio, live. Null keeps its own revisions.
    #[serde(default)]
    pub portfolio_id: Option<Uuid>,
    #[serde(default)]
    pub review_days: i32,
}

fn one() -> i16 {
    1
}

fn default_currency() -> String {
    "USD".to_string()
}

#[derive(Debug, Default, Deserialize)]
pub struct AssetFilter {
    /// Empty = no filter; otherwise the asset must match one of these.
    pub asset_types: Vec<String>,
    /// Same, over the category ("" matches an uncategorized asset).
    pub categories: Vec<String>,
}

pub async fn list_assets(pool: &PgPool, filter: &AssetFilter) -> anyhow::Result<Vec<Asset>> {
    // Latest revision per asset via a lateral join.
    let sql = "SELECT a.id, a.template_id, a.name, a.asset_type, a.currency, a.category, \
                      a.sign, a.portfolio_id, a.review_days, \
                      r.value, r.valued_at, \
                      (SELECT COUNT(*) FROM wealth_revisions x WHERE x.asset_id = a.id) AS rc \
               FROM wealth_assets a \
               LEFT JOIN LATERAL ( \
                   SELECT value, valued_at FROM wealth_revisions \
                   WHERE asset_id = a.id ORDER BY valued_at DESC, created_at DESC LIMIT 1 \
               ) r ON TRUE \
               WHERE (cardinality($1::text[]) = 0 OR a.asset_type = ANY($1)) \
                 AND (cardinality($2::text[]) = 0 OR COALESCE(a.category, '') = ANY($2)) \
               ORDER BY a.name, a.created_at";
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        template_id: Option<Uuid>,
        name: String,
        asset_type: String,
        currency: String,
        category: Option<String>,
        sign: i16,
        portfolio_id: Option<Uuid>,
        review_days: i32,
        value: Option<f64>,
        valued_at: Option<Date>,
        rc: i64,
    }
    let rows = sqlx::query_as::<_, Row>(sql)
        .bind(&filter.asset_types)
        .bind(&filter.categories)
        .fetch_all(pool)
        .await
        .context("listing assets")?;
    Ok(rows
        .into_iter()
        .map(|r| Asset {
            id: r.id,
            template_id: r.template_id,
            name: r.name,
            asset_type: r.asset_type,
            currency: r.currency,
            category: r.category,
            sign: r.sign,
            portfolio_id: r.portfolio_id,
            review_days: r.review_days,
            latest_value: r.value,
            latest_at: r.valued_at,
            revision_count: r.rc,
        })
        .collect())
}

pub async fn add_asset(pool: &PgPool, input: &AssetInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wealth_assets \
           (id, template_id, name, asset_type, currency, category, sign, portfolio_id, review_days) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(input.template_id)
    .bind(&input.name)
    .bind(&input.asset_type)
    .bind(&input.currency)
    .bind(input.category.as_deref())
    .bind(input.sign)
    .bind(input.portfolio_id)
    .bind(input.review_days)
    .execute(pool)
    .await
    .context("inserting asset")?;
    Ok(id)
}

pub async fn update_asset(pool: &PgPool, id: Uuid, input: &AssetInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE wealth_assets SET \
            template_id = $2, name = $3, asset_type = $4, currency = $5, category = $6, \
            sign = $7, portfolio_id = $8, review_days = $9, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(input.template_id)
    .bind(&input.name)
    .bind(&input.asset_type)
    .bind(&input.currency)
    .bind(input.category.as_deref())
    .bind(input.sign)
    .bind(input.portfolio_id)
    .bind(input.review_days)
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
    /// What is owned and what is owed, before they are netted. A net worth that does not
    /// show both is a number the reader cannot check.
    pub assets_value: f64,
    pub liabilities_value: f64,
    /// Assets whose valuation is older than their own review interval. A house valued three
    /// years ago is wrong in silence, and silence is the failure mode this names.
    pub stale: Vec<StaleAsset>,
}

/// A valuation that has aged past what its owner asked for.
#[derive(Debug, Serialize)]
pub struct StaleAsset {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "date_opt")]
    pub valued_at: Option<Date>,
    /// Days since the last valuation, or None when there has never been one.
    pub age_days: Option<i64>,
    pub review_days: i32,
}

/// Each asset's latest revision value at or before a given date, in the asset's currency.
struct AssetValue {
    currency: String,
    category: String,
    /// +1 owned, -1 owed.
    sign: f64,
    /// (valued_at, value) revisions sorted ascending by date. For a linked asset these are
    /// the portfolio's own daily snapshots, which is the same history read from the module
    /// that owns it rather than a copy that goes stale the next morning.
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

/// The daily history of a linked portfolio, as (date, net worth) in its own currency.
///
/// Read from `portfolio_snapshots`, which is the portfolio module's own record: MyWealth
/// never keeps a second copy of a number another module already owns.
async fn linked_history(
    pool: &PgPool,
    portfolio_id: Uuid,
) -> anyhow::Result<Option<(String, Vec<(Date, f64)>)>> {
    let Some(pf) = crate::portfolios::get_portfolio(pool, portfolio_id).await? else {
        return Ok(None);
    };
    let snaps = crate::portfolios::list_snapshots(pool, portfolio_id).await?;
    let mut history: Vec<(Date, f64)> = snaps
        .iter()
        .map(|s| (s.snap_date, s.market_value + s.cash))
        .collect();
    // Today's figure comes from the live book, not from the last snapshot: a portfolio that
    // has not been refreshed today would otherwise pull the whole net worth back in time.
    let book = crate::portfolios::book(pool, &pf).await?;
    let today = OffsetDateTime::now_utc().date();
    match history.last_mut() {
        Some((d, v)) if *d == today => *v = book.net_worth,
        _ => history.push((today, book.net_worth)),
    }
    Ok(Some((pf.currency, history)))
}

/// Valuations older than the interval their owner asked for.
fn stale_assets(assets: &[Asset], today: Date) -> Vec<StaleAsset> {
    let mut out: Vec<StaleAsset> = assets
        .iter()
        // A linked asset is valued live and can never go stale, so it is not nagged about.
        .filter(|a| a.review_days > 0 && a.portfolio_id.is_none())
        .filter_map(|a| {
            let age = a.latest_at.map(|d| (today - d).whole_days());
            // Never valued counts as stale: that is the worst case, not an exempt one.
            match age {
                Some(days) if days <= a.review_days as i64 => None,
                _ => Some(StaleAsset {
                    id: a.id,
                    name: a.name.clone(),
                    valued_at: a.latest_at,
                    age_days: age,
                    review_days: a.review_days,
                }),
            }
        })
        .collect();
    // Oldest first: the one most likely to be wrong is the one to look at.
    out.sort_by(|a, b| b.age_days.unwrap_or(i64::MAX).cmp(&a.age_days.unwrap_or(i64::MAX)));
    out
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
        // A linked asset is valued from the portfolio it points at, never from a copy: the
        // whole reason the link exists is that a copied market value is wrong tomorrow.
        let (currency, revisions) = match a.portfolio_id {
            Some(pf_id) => match linked_history(pool, pf_id).await? {
                Some(v) => v,
                // The portfolio is gone or has no history: fall back to whatever revisions
                // the asset kept, rather than dropping it out of the net worth entirely.
                None => (a.currency.clone(), revs),
            },
            None => (a.currency.clone(), revs),
        };
        values.push(AssetValue {
            currency,
            category: a.category.clone().unwrap_or_default(),
            sign: a.sign as f64,
            revisions,
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
    let mut assets_value = 0.0;
    let mut liabilities_value = 0.0;
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
                        let signed = v * av.sign;
                        sum += signed;
                        if is_latest {
                            *by_category.entry(av.category.clone()).or_insert(0.0) += signed;
                            if av.sign < 0.0 {
                                liabilities_value += v;
                            } else {
                                assets_value += v;
                            }
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

    trim_leading_empty(&mut points);

    let net_worth = points.last().map(|p| p.net_worth).unwrap_or(0.0);
    Ok(Breakdown {
        display_currency: display_currency.to_string(),
        net_worth,
        asset_count: assets.len() as i64,
        points,
        by_category,
        unconverted,
        assets_value,
        liabilities_value,
        stale: stale_assets(&assets, today),
    })
}

/// Drop the run of samples that predate the first revision, keeping the last of them as a
/// zero baseline. Without this a book started this year plots as several flat-zero periods
/// and one spike, squashing the real variation into the last pixel. All-zero series (no
/// revisions at all) are left alone.
fn trim_leading_empty(points: &mut Vec<NetWorthPoint>) {
    if let Some(first_data) = points.iter().position(|p| p.net_worth != 0.0) {
        points.drain(..first_data.saturating_sub(1));
    }
}

/// Dec 31 of the year `n` years before `from`, never in the future: the current year's
/// "year end" is `from` itself, so the series doesn't run past today (which would sample
/// dates with no data and, with `today` appended after them, walk the x axis backwards).
fn year_end_back(from: Date, n: i64) -> Date {
    let year = from.year() - n as i32;
    let end = Date::from_calendar_date(year, time::Month::December, 31).unwrap_or(from);
    end.min(from)
}

/// The last day of the month `n` months before `from`, never in the future (see
/// [`year_end_back`]: the current month's end has not happened yet).
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
    next.previous_day().unwrap_or(first).min(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn d(y: i32, m: Month, day: u8) -> Date {
        Date::from_calendar_date(y, m, day).unwrap()
    }

    // The current period's end has not happened yet: sampling it produced a future date, and
    // since `today` is appended after the samples the x axis then walked backwards.
    #[test]
    fn period_ends_are_never_in_the_future() {
        let today = d(2026, Month::August, 13);
        assert_eq!(year_end_back(today, 0), today);
        assert_eq!(month_end_back(today, 0), today);
    }

    #[test]
    fn past_period_ends_are_untouched() {
        let today = d(2026, Month::August, 13);
        assert_eq!(year_end_back(today, 1), d(2025, Month::December, 31));
        assert_eq!(month_end_back(today, 1), d(2026, Month::July, 31));
        // Leap February, two years back.
        assert_eq!(month_end_back(d(2025, Month::March, 5), 13), d(2024, Month::February, 29));
    }

    #[test]
    fn leading_empty_periods_are_trimmed_to_one_baseline() {
        let mk = |vals: &[f64]| -> Vec<NetWorthPoint> {
            vals.iter()
                .enumerate()
                .map(|(i, v)| NetWorthPoint { at: d(2020 + i as i32, Month::December, 31), net_worth: *v })
                .collect()
        };

        let mut p = mk(&[0.0, 0.0, 0.0, 5.0, 7.0]);
        trim_leading_empty(&mut p);
        // One zero kept as the baseline the rise starts from.
        assert_eq!(p.iter().map(|x| x.net_worth).collect::<Vec<_>>(), vec![0.0, 5.0, 7.0]);

        // Nothing to trim.
        let mut p = mk(&[3.0, 4.0]);
        trim_leading_empty(&mut p);
        assert_eq!(p.len(), 2);

        // An empty book keeps its series rather than collapsing to nothing.
        let mut p = mk(&[0.0, 0.0]);
        trim_leading_empty(&mut p);
        assert_eq!(p.len(), 2);

        // A zero *after* data is real (net worth went to zero) and must survive.
        let mut p = mk(&[0.0, 9.0, 0.0]);
        trim_leading_empty(&mut p);
        assert_eq!(p.iter().map(|x| x.net_worth).collect::<Vec<_>>(), vec![0.0, 9.0, 0.0]);
    }

    // Carry-forward: the latest revision on/before the sample date.
    #[test]
    fn value_on_carries_forward() {
        let av = AssetValue {
            currency: "USD".into(),
            category: String::new(),
            sign: 1.0,
            revisions: vec![(d(2026, Month::January, 1), 100.0), (d(2026, Month::August, 13), 161.0)],
        };
        assert_eq!(av.value_on(d(2025, Month::December, 31)), None);
        assert_eq!(av.value_on(d(2026, Month::January, 1)), Some(100.0));
        assert_eq!(av.value_on(d(2026, Month::July, 31)), Some(100.0));
        assert_eq!(av.value_on(d(2026, Month::August, 13)), Some(161.0));
    }
}

#[cfg(test)]
mod liability_tests {
    use super::*;
    use time::Month;

    fn asset(name: &str, review_days: i32, valued_days_ago: Option<i64>, linked: bool) -> Asset {
        let today = time::Date::from_calendar_date(2026, Month::September, 4).unwrap();
        Asset {
            id: Uuid::nil(),
            template_id: None,
            name: name.into(),
            asset_type: "house".into(),
            currency: "EUR".into(),
            category: None,
            sign: 1,
            portfolio_id: linked.then(Uuid::new_v4),
            review_days,
            latest_value: Some(1.0),
            latest_at: valued_days_ago.map(|n| today - time::Duration::days(n)),
            revision_count: 1,
        }
    }

    fn today() -> Date {
        time::Date::from_calendar_date(2026, Month::September, 4).unwrap()
    }

    #[test]
    fn a_fresh_valuation_is_not_stale() {
        let rows = stale_assets(&[asset("house", 365, Some(30), false)], today());
        assert!(rows.is_empty());
    }

    #[test]
    fn an_aged_valuation_is_named_with_its_age() {
        let rows = stale_assets(&[asset("house", 365, Some(400), false)], today());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].age_days, Some(400));
        assert_eq!(rows[0].review_days, 365);
    }

    /// Never valued is the worst case, not an exempt one.
    #[test]
    fn never_valued_counts_as_stale() {
        let rows = stale_assets(&[asset("watch", 90, None, false)], today());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].age_days, None);
    }

    /// A linked asset is read live, so it cannot age. Nagging about it would be noise the
    /// user can do nothing about.
    #[test]
    fn a_linked_asset_never_goes_stale() {
        let rows = stale_assets(&[asset("broker", 30, Some(999), true)], today());
        assert!(rows.is_empty());
    }

    #[test]
    fn zero_means_never_nag() {
        let rows = stale_assets(&[asset("cash", 0, Some(9999), false)], today());
        assert!(rows.is_empty());
    }

    /// Oldest first: the one most likely to be wrong is the one to look at.
    #[test]
    fn the_oldest_is_reported_first() {
        let rows = stale_assets(
            &[
                asset("recent", 10, Some(50), false),
                asset("ancient", 10, Some(900), false),
                asset("never", 10, None, false),
            ],
            today(),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "never");
        assert_eq!(rows[1].name, "ancient");
    }
}
