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
///
/// `broker_account` and `wallet` are read live like a linked portfolio: a broker account's
/// equity, a public address's balance on chain. Each reading is kept as that day's revision,
/// so the curve has a history and a failed read leaves the last known value with its date.
pub const ASSET_TYPES: [&str; 11] = [
    "money", "portfolio", "loan", "watch", "house", "vehicle", "other", "stock", "crypto",
    "broker_account", "wallet",
];

/// The kinds whose value comes from a market or a venue rather than from the owner. Their
/// change over a period is market effect, not saving, which is what the savings rate needs
/// to tell apart.
pub const MARKET_TYPES: [&str; 5] = ["portfolio", "broker_account", "wallet", "stock", "crypto"];

/// `fields.source` on a revision the module wrote itself: a loan's monthly balance or a live
/// reading. A hand-entered revision carries no source.
pub const SOURCE_LOAN: &str = "loan";
pub const SOURCE_LIVE: &str = "live";

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
    /// A liability's terms, `{ principal, rate_pct, term_months, start, payment }`.
    pub loan: Option<JsonValue>,
    /// The debt held against this asset, for equity and loan-to-value.
    pub paired_liability_id: Option<Uuid>,
    /// Assumed yearly change applied on read after the last revision, in percent.
    pub drift_pct_year: Option<f64>,
    /// A broker account whose equity is this asset's value.
    pub broker_account_id: Option<Uuid>,
    /// `btc` | `eth` for a wallet address.
    pub chain: Option<String>,
    pub address: Option<String>,
    /// Household member the asset belongs to.
    pub owner: Option<String>,
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
    /// Loan terms for a liability; null for anything else.
    #[serde(default)]
    pub loan: Option<JsonValue>,
    #[serde(default)]
    pub paired_liability_id: Option<Uuid>,
    #[serde(default)]
    pub drift_pct_year: Option<f64>,
    #[serde(default)]
    pub broker_account_id: Option<Uuid>,
    #[serde(default)]
    pub chain: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
}

impl Asset {
    /// Valued by the module itself (a linked portfolio, a broker account, a wallet, a loan's
    /// schedule): nothing for the owner to revisit, so never stale.
    pub fn is_automatic(&self) -> bool {
        self.portfolio_id.is_some()
            || self.broker_account_id.is_some()
            || self.address.is_some()
            || self.loan.is_some()
    }
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
    /// Same, over the owner ("" matches an asset with no owner).
    pub owners: Vec<String>,
}

const ASSET_COLS: &str = "a.id, a.template_id, a.name, a.asset_type, a.currency, a.category, \
     a.sign, a.portfolio_id, a.review_days, a.loan, a.paired_liability_id, a.drift_pct_year, \
     a.broker_account_id, a.chain, a.address, a.owner";

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: Uuid,
    template_id: Option<Uuid>,
    name: String,
    asset_type: String,
    currency: String,
    category: Option<String>,
    sign: i16,
    portfolio_id: Option<Uuid>,
    review_days: i32,
    loan: Option<JsonValue>,
    paired_liability_id: Option<Uuid>,
    drift_pct_year: Option<f64>,
    broker_account_id: Option<Uuid>,
    chain: Option<String>,
    address: Option<String>,
    owner: Option<String>,
    value: Option<f64>,
    valued_at: Option<Date>,
    rc: i64,
}

impl From<AssetRow> for Asset {
    fn from(r: AssetRow) -> Self {
        Asset {
            id: r.id,
            template_id: r.template_id,
            name: r.name,
            asset_type: r.asset_type,
            currency: r.currency,
            category: r.category,
            sign: r.sign,
            portfolio_id: r.portfolio_id,
            review_days: r.review_days,
            loan: r.loan,
            paired_liability_id: r.paired_liability_id,
            drift_pct_year: r.drift_pct_year,
            broker_account_id: r.broker_account_id,
            chain: r.chain,
            address: r.address,
            owner: r.owner,
            latest_value: r.value,
            latest_at: r.valued_at,
            revision_count: r.rc,
        }
    }
}

pub async fn list_assets(pool: &PgPool, filter: &AssetFilter) -> anyhow::Result<Vec<Asset>> {
    // Latest revision per asset via a lateral join.
    let sql = format!(
        "SELECT {ASSET_COLS}, r.value, r.valued_at, \
                (SELECT COUNT(*) FROM wealth_revisions x WHERE x.asset_id = a.id) AS rc \
         FROM wealth_assets a \
         LEFT JOIN LATERAL ( \
             SELECT value, valued_at FROM wealth_revisions \
             WHERE asset_id = a.id ORDER BY valued_at DESC, created_at DESC LIMIT 1 \
         ) r ON TRUE \
         WHERE (cardinality($1::text[]) = 0 OR a.asset_type = ANY($1)) \
           AND (cardinality($2::text[]) = 0 OR COALESCE(a.category, '') = ANY($2)) \
           AND (cardinality($3::text[]) = 0 OR COALESCE(a.owner, '') = ANY($3)) \
         ORDER BY a.name, a.created_at"
    );
    let rows = sqlx::query_as::<_, AssetRow>(sqlx::AssertSqlSafe(sql))
        .bind(&filter.asset_types)
        .bind(&filter.categories)
        .bind(&filter.owners)
        .fetch_all(pool)
        .await
        .context("listing assets")?;
    Ok(rows.into_iter().map(Asset::from).collect())
}

pub async fn get_asset(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Asset>> {
    let sql = format!(
        "SELECT {ASSET_COLS}, r.value, r.valued_at, \
                (SELECT COUNT(*) FROM wealth_revisions x WHERE x.asset_id = a.id) AS rc \
         FROM wealth_assets a \
         LEFT JOIN LATERAL ( \
             SELECT value, valued_at FROM wealth_revisions \
             WHERE asset_id = a.id ORDER BY valued_at DESC, created_at DESC LIMIT 1 \
         ) r ON TRUE \
         WHERE a.id = $1"
    );
    let row = sqlx::query_as::<_, AssetRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading asset")?;
    Ok(row.map(Asset::from))
}

pub async fn add_asset(pool: &PgPool, input: &AssetInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wealth_assets \
           (id, template_id, name, asset_type, currency, category, sign, portfolio_id, review_days, \
            loan, paired_liability_id, drift_pct_year, broker_account_id, chain, address, owner) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
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
    .bind(input.loan.as_ref())
    .bind(input.paired_liability_id)
    .bind(input.drift_pct_year)
    .bind(input.broker_account_id)
    .bind(input.chain.as_deref())
    .bind(input.address.as_deref())
    .bind(input.owner.as_deref())
    .execute(pool)
    .await
    .context("inserting asset")?;
    Ok(id)
}

pub async fn update_asset(pool: &PgPool, id: Uuid, input: &AssetInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE wealth_assets SET \
            template_id = $2, name = $3, asset_type = $4, currency = $5, category = $6, \
            sign = $7, portfolio_id = $8, review_days = $9, loan = $10, \
            paired_liability_id = $11, drift_pct_year = $12, broker_account_id = $13, \
            chain = $14, address = $15, owner = $16, updated_at = now() \
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
    .bind(input.loan.as_ref())
    .bind(input.paired_liability_id)
    .bind(input.drift_pct_year)
    .bind(input.broker_account_id)
    .bind(input.chain.as_deref())
    .bind(input.address.as_deref())
    .bind(input.owner.as_deref())
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

/// Write the module's own revision for `date`: replaced when one from the same source is
/// already there for that day, inserted otherwise. A hand-entered revision on the same day is
/// left alone (it carries no source), and the later of the two wins on read as usual.
pub async fn put_auto_revision(
    pool: &PgPool,
    asset_id: Uuid,
    date: Date,
    value: f64,
    source: &str,
    note: &str,
) -> anyhow::Result<()> {
    let updated = sqlx::query(
        "UPDATE wealth_revisions SET value = $3, note = $5, created_at = now() \
         WHERE asset_id = $1 AND valued_at = $2 AND fields->>'source' = $4",
    )
    .bind(asset_id)
    .bind(date)
    .bind(value)
    .bind(source)
    .bind(note)
    .execute(pool)
    .await
    .context("updating automatic revision")?;
    if updated.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO wealth_revisions (id, asset_id, valued_at, value, fields, note) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(asset_id)
        .bind(date)
        .bind(value)
        .bind(serde_json::json!({ "source": source }))
        .bind(note)
        .execute(pool)
        .await
        .context("inserting automatic revision")?;
    }
    Ok(())
}

/// Replace every revision `source` wrote for this asset with `rows`. A loan whose terms were
/// corrected rewrites its whole schedule; the owner's own revisions are kept.
pub async fn replace_auto_revisions(
    pool: &PgPool,
    asset_id: Uuid,
    source: &str,
    rows: &[(Date, f64)],
    note: &str,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM wealth_revisions WHERE asset_id = $1 AND fields->>'source' = $2")
        .bind(asset_id)
        .bind(source)
        .execute(&mut *tx)
        .await
        .context("clearing automatic revisions")?;
    for (date, value) in rows {
        sqlx::query(
            "INSERT INTO wealth_revisions (id, asset_id, valued_at, value, fields, note) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(asset_id)
        .bind(*date)
        .bind(*value)
        .bind(serde_json::json!({ "source": source }))
        .bind(note)
        .execute(&mut *tx)
        .await
        .context("inserting automatic revision")?;
    }
    tx.commit().await?;
    Ok(())
}

/// Latest revision date `source` wrote for this asset, if any.
pub async fn last_auto_revision(pool: &PgPool, asset_id: Uuid, source: &str) -> anyhow::Result<Option<Date>> {
    let row: (Option<Date>,) = sqlx::query_as(
        "SELECT MAX(valued_at) FROM wealth_revisions WHERE asset_id = $1 AND fields->>'source' = $2",
    )
    .bind(asset_id)
    .bind(source)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Every revision of every asset, oldest first per asset, for the CSV export.
pub async fn all_revisions(pool: &PgPool) -> anyhow::Result<Vec<Revision>> {
    let rows = sqlx::query_as::<
        _,
        (Uuid, Uuid, Date, Option<f64>, Option<f64>, f64, JsonValue, Option<String>),
    >(
        "SELECT id, asset_id, valued_at, price, quantity, value, fields, note \
         FROM wealth_revisions ORDER BY asset_id, valued_at, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing all revisions")?;
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

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Settings {
    pub display_currency: String,
    /// Target net worth in the display currency.
    pub goal_value: Option<f64>,
    #[serde(with = "date_opt")]
    pub goal_date: Option<Date>,
    /// What a month costs, in the display currency.
    pub monthly_expenses: Option<f64>,
    /// `off` | `weekly`.
    pub stale_notify: String,
    /// Channel ids the stale notice is narrowed to; empty = every channel granted to wealth.
    pub stale_channels: Vec<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub stale_last_at: Option<OffsetDateTime>,
    /// Fundamentals series deflating the curve; None = US CPI when loaded.
    pub cpi_series_id: Option<Uuid>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            display_currency: "USD".to_string(),
            goal_value: None,
            goal_date: None,
            monthly_expenses: None,
            stale_notify: "off".to_string(),
            stale_channels: Vec::new(),
            stale_last_at: None,
            cpi_series_id: None,
        }
    }
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    #[derive(sqlx::FromRow)]
    struct Row {
        display_currency: String,
        goal_value: Option<f64>,
        goal_date: Option<Date>,
        monthly_expenses: Option<f64>,
        stale_notify: String,
        stale_channels: JsonValue,
        stale_last_at: Option<OffsetDateTime>,
        cpi_series_id: Option<Uuid>,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT display_currency, goal_value, goal_date, monthly_expenses, stale_notify, \
                stale_channels, stale_last_at, cpi_series_id \
         FROM wealth_settings WHERE id = TRUE",
    )
    .fetch_optional(pool)
    .await
    .context("loading wealth settings")?;
    Ok(match row {
        None => Settings::default(),
        Some(r) => Settings {
            display_currency: r.display_currency,
            goal_value: r.goal_value,
            goal_date: r.goal_date,
            monthly_expenses: r.monthly_expenses,
            stale_notify: r.stale_notify,
            stale_channels: serde_json::from_value(r.stale_channels).unwrap_or_default(),
            stale_last_at: r.stale_last_at,
            cpi_series_id: r.cpi_series_id,
        },
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

/// Write the goal, stale-notice and CPI settings as a whole: the settings form sends every
/// field, and a cleared field is a cleared setting.
pub async fn set_preferences(pool: &PgPool, s: &Settings) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO wealth_settings (id, goal_value, goal_date, monthly_expenses, stale_notify, \
                                      stale_channels, cpi_series_id, updated_at) \
         VALUES (TRUE, $1, $2, $3, $4, $5, $6, now()) \
         ON CONFLICT (id) DO UPDATE SET goal_value = $1, goal_date = $2, monthly_expenses = $3, \
             stale_notify = $4, stale_channels = $5, cpi_series_id = $6, updated_at = now()",
    )
    .bind(s.goal_value)
    .bind(s.goal_date)
    .bind(s.monthly_expenses)
    .bind(&s.stale_notify)
    .bind(serde_json::json!(s.stale_channels))
    .bind(s.cpi_series_id)
    .execute(pool)
    .await
    .context("saving wealth preferences")?;
    Ok(())
}

/// Record that the weekly stale pass ran.
pub async fn mark_stale_run(pool: &PgPool, at: OffsetDateTime) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO wealth_settings (id, stale_last_at, updated_at) VALUES (TRUE, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET stale_last_at = $1",
    )
    .bind(at)
    .execute(pool)
    .await
    .context("recording stale pass")?;
    Ok(())
}

// ── Categories and targets ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Category {
    pub name: String,
    pub liquid: bool,
}

/// Categories carrying a flag. A category in use with no row is illiquid.
pub async fn list_categories(pool: &PgPool) -> anyhow::Result<Vec<Category>> {
    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT name, liquid FROM wealth_categories ORDER BY name")
            .fetch_all(pool)
            .await
            .context("listing wealth categories")?;
    Ok(rows.into_iter().map(|(name, liquid)| Category { name, liquid }).collect())
}

pub async fn set_category_liquid(pool: &PgPool, name: &str, liquid: bool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO wealth_categories (name, liquid, updated_at) VALUES ($1, $2, now()) \
         ON CONFLICT (name) DO UPDATE SET liquid = $2, updated_at = now()",
    )
    .bind(name)
    .bind(liquid)
    .execute(pool)
    .await
    .context("saving wealth category")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Target {
    /// Category name; "" is the uncategorized bucket.
    pub category: String,
    pub target_pct: f64,
    pub band_pct: f64,
}

pub async fn list_targets(pool: &PgPool) -> anyhow::Result<Vec<Target>> {
    let rows: Vec<(String, f64, f64)> =
        sqlx::query_as("SELECT category, target_pct, band_pct FROM wealth_targets ORDER BY category")
            .fetch_all(pool)
            .await
            .context("listing wealth targets")?;
    Ok(rows
        .into_iter()
        .map(|(category, target_pct, band_pct)| Target { category, target_pct, band_pct })
        .collect())
}

/// Replace the whole target set: the modal edits it as one plan.
pub async fn replace_targets(pool: &PgPool, targets: &[Target]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM wealth_targets").execute(&mut *tx).await?;
    for t in targets {
        sqlx::query("INSERT INTO wealth_targets (category, target_pct, band_pct) VALUES ($1,$2,$3)")
            .bind(&t.category)
            .bind(t.target_pct)
            .bind(t.band_pct)
            .execute(&mut *tx)
            .await
            .context("inserting wealth target")?;
    }
    tx.commit().await?;
    Ok(())
}

// ── Attachments ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub revision_id: Option<Uuid>,
    pub name: String,
    pub mime: String,
    pub size: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

type AttachmentRow = (Uuid, Uuid, Option<Uuid>, String, String, i32, OffsetDateTime);

fn attachment((id, asset_id, revision_id, name, mime, size, created_at): AttachmentRow) -> Attachment {
    Attachment { id, asset_id, revision_id, name, mime, size, created_at }
}

pub async fn list_attachments(pool: &PgPool, asset_id: Uuid) -> anyhow::Result<Vec<Attachment>> {
    let rows = sqlx::query_as::<_, AttachmentRow>(
        "SELECT id, asset_id, revision_id, name, mime, size, created_at FROM wealth_attachments \
         WHERE asset_id = $1 ORDER BY created_at DESC",
    )
    .bind(asset_id)
    .fetch_all(pool)
    .await
    .context("listing wealth attachments")?;
    Ok(rows.into_iter().map(attachment).collect())
}

pub async fn add_attachment(
    pool: &PgPool,
    asset_id: Uuid,
    revision_id: Option<Uuid>,
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wealth_attachments (id, asset_id, revision_id, name, mime, bytes, size) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(asset_id)
    .bind(revision_id)
    .bind(name)
    .bind(mime)
    .bind(bytes)
    .bind(bytes.len() as i32)
    .execute(pool)
    .await
    .context("inserting wealth attachment")?;
    Ok(id)
}

/// The attachment and its bytes.
pub async fn get_attachment(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<(Attachment, Vec<u8>)>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>, String, String, i32, OffsetDateTime, Vec<u8>)>(
        "SELECT id, asset_id, revision_id, name, mime, size, created_at, bytes \
         FROM wealth_attachments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("loading wealth attachment")?;
    Ok(row.map(|(id, asset_id, revision_id, name, mime, size, created_at, bytes)| {
        (attachment((id, asset_id, revision_id, name, mime, size, created_at)), bytes)
    }))
}

pub async fn delete_attachment(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM wealth_attachments WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Net-worth breakdown ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NetWorthPoint {
    #[serde(with = "date_iso")]
    pub at: Date,
    pub net_worth: f64,
    /// The part of `net_worth` that is an assumed drift since the last real revision, signed.
    /// Zero when every value at this date is a recorded one.
    #[serde(default)]
    pub estimated: f64,
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
    /// Same, per owner ("" = no owner).
    pub by_owner: std::collections::BTreeMap<String, f64>,
    /// Assets excluded from a point because no FX rate was available for their currency.
    pub unconverted: i64,
    /// What is owned and what is owed, before they are netted. A net worth that does not
    /// show both is a number the reader cannot check.
    pub assets_value: f64,
    pub liabilities_value: f64,
    /// The part of the headline that is an assumed drift, not a recorded value.
    pub estimated_value: f64,
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
pub struct AssetValue {
    pub currency: String,
    pub category: String,
    /// +1 owned, -1 owed.
    pub sign: f64,
    /// (valued_at, value) revisions sorted ascending by date. For a linked asset these are
    /// the portfolio's own daily snapshots, which is the same history read from the module
    /// that owns it rather than a copy that goes stale the next morning.
    pub revisions: Vec<(Date, f64)>,
    /// Assumed yearly change after the last revision, in percent.
    pub drift_pct_year: Option<f64>,
}

impl AssetValue {
    #[cfg(test)]
    fn value_on(&self, date: Date) -> Option<f64> {
        self.estimate_on(date).map(|(v, _)| v)
    }

    /// The value at `date` and the part of it that is drift: the latest revision on or before
    /// the date, compounded at the assumed yearly rate for the days since.
    pub fn estimate_on(&self, date: Date) -> Option<(f64, f64)> {
        let (at, v) = self.revisions.iter().rev().find(|(d, _)| *d <= date).copied()?;
        match self.drift_pct_year {
            Some(rate) if rate != 0.0 && date > at => {
                let est = drifted(v, rate, (date - at).whole_days());
                Some((est, est - v))
            }
            _ => Some((v, 0.0)),
        }
    }
}

/// `value` compounded at `pct_year` percent a year over `days`. A car losing 15 % a year is
/// worth 85 % after 365.25 days, and the same rate applies to any fraction of a year.
pub fn drifted(value: f64, pct_year: f64, days: i64) -> f64 {
    let growth = 1.0 + pct_year / 100.0;
    if growth <= 0.0 {
        // -100 % a year or worse: worthless after the first day, never negative.
        return 0.0;
    }
    value * growth.powf(days as f64 / 365.25)
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
        // An asset the module values itself (linked, live, a loan schedule) can never go
        // stale, so it is not nagged about.
        .filter(|a| a.review_days > 0 && !a.is_automatic())
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

/// Every valuation that has aged past its interval, across the whole book.
pub async fn stale_now(pool: &PgPool) -> anyhow::Result<Vec<StaleAsset>> {
    let assets = list_assets(pool, &AssetFilter::default()).await?;
    Ok(stale_assets(&assets, OffsetDateTime::now_utc().date()))
}

/// Load each asset's value history: its revisions, or the linked portfolio's own snapshots.
pub async fn load_values(pool: &PgPool, assets: &[Asset]) -> anyhow::Result<Vec<AssetValue>> {
    let mut values: Vec<AssetValue> = Vec::new();
    for a in assets {
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
            // Drift is an owner's assumption about a thing they value themselves. Anything
            // the module reads or computes has a real figure and is never extrapolated.
            drift_pct_year: if a.is_automatic() { None } else { a.drift_pct_year },
        });
    }
    Ok(values)
}

/// One asset's value at a date, in the display currency.
#[derive(Debug, Clone, Serialize)]
pub struct AssetAt {
    pub id: Uuid,
    pub name: String,
    pub asset_type: String,
    pub category: String,
    pub owner: String,
    /// +1 owned, -1 owed.
    pub sign: i16,
    /// Signed value in the display currency (a debt is negative). None = no revision yet on
    /// that date, or no FX rate (then `unconverted` is true).
    pub value: Option<f64>,
    /// Signed part of `value` that is drift.
    pub estimated: f64,
    pub unconverted: bool,
}

/// Every asset in scope valued at `date`: the latest revision on or before it, the way the
/// chart samples a point.
pub async fn values_at(
    pool: &PgPool,
    assets: &[Asset],
    values: &[AssetValue],
    date: Date,
    display_currency: &str,
) -> anyhow::Result<Vec<AssetAt>> {
    let mut out = Vec::with_capacity(assets.len());
    for (a, av) in assets.iter().zip(values) {
        let mut row = AssetAt {
            id: a.id,
            name: a.name.clone(),
            asset_type: a.asset_type.clone(),
            category: av.category.clone(),
            owner: a.owner.clone().unwrap_or_default(),
            sign: a.sign,
            value: None,
            estimated: 0.0,
            unconverted: false,
        };
        if let Some((native, est)) = av.estimate_on(date) {
            match crate::journal_fx::convert(pool, native, &av.currency, display_currency, date).await? {
                Some(v) => {
                    row.value = Some(v * av.sign);
                    // The drift share converts at the same rate as the whole.
                    row.estimated = if native != 0.0 { v * av.sign * est / native } else { 0.0 };
                }
                None => row.unconverted = true,
            }
        }
        out.push(row);
    }
    Ok(out)
}

/// Net worth per category in a set of valued assets.
pub fn by_category(rows: &[AssetAt]) -> std::collections::BTreeMap<String, f64> {
    let mut map = std::collections::BTreeMap::new();
    for r in rows {
        if let Some(v) = r.value {
            *map.entry(r.category.clone()).or_insert(0.0) += v;
        }
    }
    map
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
    let assets = list_assets(pool, filter).await?;
    let values = load_values(pool, &assets).await?;

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
    let mut latest: Vec<AssetAt> = Vec::new();
    for d in &dates {
        let rows = values_at(pool, &assets, &values, *d, display_currency).await?;
        let net_worth = rows.iter().filter_map(|r| r.value).sum();
        let estimated = rows.iter().map(|r| r.estimated).sum();
        points.push(NetWorthPoint { at: *d, net_worth, estimated });
        latest = rows;
    }

    trim_leading_empty(&mut points);

    // Everything the headline says is read from the latest sample; an unconverted asset is
    // counted once there rather than once per point.
    let mut by_owner = std::collections::BTreeMap::new();
    let (mut assets_value, mut liabilities_value) = (0.0, 0.0);
    for r in &latest {
        if let Some(v) = r.value {
            *by_owner.entry(r.owner.clone()).or_insert(0.0) += v;
            if r.sign < 0 {
                liabilities_value -= v;
            } else {
                assets_value += v;
            }
        }
    }
    let net_worth = points.last().map(|p| p.net_worth).unwrap_or(0.0);
    let estimated_value = points.last().map(|p| p.estimated).unwrap_or(0.0);
    Ok(Breakdown {
        display_currency: display_currency.to_string(),
        net_worth,
        asset_count: assets.len() as i64,
        points,
        by_category: by_category(&latest),
        by_owner,
        unconverted: latest.iter().filter(|r| r.unconverted).count() as i64,
        assets_value,
        liabilities_value,
        estimated_value,
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
pub fn year_end_back(from: Date, n: i64) -> Date {
    let year = from.year() - n as i32;
    let end = Date::from_calendar_date(year, time::Month::December, 31).unwrap_or(from);
    end.min(from)
}

/// The last day of the month `n` months before `from`, never in the future (see
/// [`year_end_back`]: the current month's end has not happened yet).
pub fn month_end_back(from: Date, n: i64) -> Date {
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
                .map(|(i, v)| NetWorthPoint { at: d(2020 + i as i32, Month::December, 31), net_worth: *v, estimated: 0.0 })
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
            drift_pct_year: None,
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
            loan: None,
            paired_liability_id: None,
            drift_pct_year: None,
            broker_account_id: None,
            chain: None,
            address: None,
            owner: None,
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

#[cfg(test)]
mod wealth_drift_tests {
    use super::*;
    use time::Month;

    fn d(y: i32, m: Month, day: u8) -> Date {
        Date::from_calendar_date(y, m, day).unwrap()
    }

    fn house(drift: Option<f64>) -> AssetValue {
        AssetValue {
            currency: "EUR".into(),
            category: "Property".into(),
            sign: 1.0,
            revisions: vec![(d(2024, Month::January, 1), 100_000.0), (d(2025, Month::January, 1), 110_000.0)],
            drift_pct_year: drift,
        }
    }

    #[test]
    fn drift_compounds_by_the_year() {
        assert!((drifted(100.0, 3.0, 365) - 100.0 * 1.03f64.powf(365.0 / 365.25)).abs() < 1e-9);
        // Two years at -15 %: 72.25 % left, not 70 %.
        assert!((drifted(100.0, -15.0, 731) - 100.0 * 0.85f64.powf(731.0 / 365.25)).abs() < 1e-9);
        assert!((drifted(100.0, -15.0, 731) - 72.25).abs() < 0.1);
        // Same day: the recorded value, untouched.
        assert_eq!(drifted(100.0, 3.0, 0), 100.0);
        // A total loss never goes negative.
        assert_eq!(drifted(100.0, -150.0, 30), 0.0);
    }

    #[test]
    fn drift_starts_at_the_last_real_revision_and_is_reported_apart() {
        let av = house(Some(3.0));
        // On a revision day the value is the recorded one, with no estimate.
        assert_eq!(av.estimate_on(d(2025, Month::January, 1)), Some((110_000.0, 0.0)));
        // A year later the estimate compounds from that revision, not from the first one.
        let (v, est) = av.estimate_on(d(2026, Month::January, 1)).unwrap();
        assert!((v - drifted(110_000.0, 3.0, 365)).abs() < 1e-6);
        assert!((est - (v - 110_000.0)).abs() < 1e-9);
        // Between two revisions the earlier one drifts until the next real value takes over.
        let (mid, mid_est) = av.estimate_on(d(2024, Month::July, 1)).unwrap();
        assert!(mid > 100_000.0 && mid_est > 0.0);
    }

    #[test]
    fn no_drift_means_carry_forward() {
        let av = house(None);
        assert_eq!(av.estimate_on(d(2030, Month::January, 1)), Some((110_000.0, 0.0)));
        assert_eq!(av.estimate_on(d(2023, Month::December, 31)), None);
    }

    /// Then versus now: a date between two revisions reads the latest one before it.
    #[test]
    fn snapshot_between_revisions_takes_the_latest_prior_one() {
        let av = house(None);
        assert_eq!(av.value_on(d(2024, Month::December, 31)), Some(100_000.0));
        assert_eq!(av.value_on(d(2025, Month::June, 30)), Some(110_000.0));
    }

    #[test]
    fn category_totals_skip_unvalued_rows() {
        let row = |cat: &str, v: Option<f64>| AssetAt {
            id: Uuid::nil(),
            name: String::new(),
            asset_type: "other".into(),
            category: cat.into(),
            owner: String::new(),
            sign: 1,
            value: v,
            estimated: 0.0,
            unconverted: false,
        };
        let m = by_category(&[row("Cash", Some(10.0)), row("Cash", Some(5.0)), row("Car", None), row("Loan", Some(-7.0))]);
        assert_eq!(m.get("Cash"), Some(&15.0));
        assert_eq!(m.get("Loan"), Some(&-7.0));
        assert!(!m.contains_key("Car"));
    }
}
