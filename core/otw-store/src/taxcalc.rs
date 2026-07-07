//! Storage for the TaxCalculator module.
//!
//! Two objects: reusable Tax Profiles (country + person type + rules as opaque JSONB) and
//! per-year Scenarios (inputs + cached computed result). The store treats `allowances`,
//! `loss_carry`, `holding_period_rules`, `wealth_tax`, `inputs` and `result` as opaque JSON —
//! the engine in otw-core owns their shape. Country rule templates are not stored here; they
//! ship as static data in otw-core. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub country: String,
    pub region: Option<String>,
    pub currency: String,
    pub person_type: String,
    pub regime: String,
    pub marginal_income_rate: Option<f64>,
    pub social_charges_rate: Option<f64>,
    pub allowances: JsonValue,
    pub loss_carry: JsonValue,
    pub holding_period_rules: JsonValue,
    pub wealth_tax: Option<JsonValue>,
    pub notes: String,
    pub is_custom: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const PROFILE_COLS: &str = "id, name, country, region, currency, person_type, regime, \
    marginal_income_rate, social_charges_rate, allowances, loss_carry, holding_period_rules, \
    wealth_tax, notes, is_custom, created_at, updated_at";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Scenario {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub name: String,
    pub tax_year: i32,
    pub mode: String,
    pub context: String,
    pub currency: String,
    pub inputs: JsonValue,
    pub result: Option<JsonValue>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const SCENARIO_COLS: &str = "id, profile_id, name, tax_year, mode, context, currency, inputs, \
    result, created_at, updated_at";

// ----- Profiles -----

pub async fn list_profiles(pool: &PgPool) -> anyhow::Result<Vec<Profile>> {
    let sql = format!("SELECT {PROFILE_COLS} FROM taxcalc_profiles ORDER BY name");
    Ok(sqlx::query_as::<_, Profile>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_profile(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Profile>> {
    let sql = format!("SELECT {PROFILE_COLS} FROM taxcalc_profiles WHERE id = $1");
    Ok(sqlx::query_as::<_, Profile>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub struct NewProfile<'a> {
    pub name: &'a str,
    pub country: &'a str,
    pub region: Option<&'a str>,
    pub currency: &'a str,
    pub person_type: &'a str,
    pub regime: &'a str,
    pub marginal_income_rate: Option<f64>,
    pub social_charges_rate: Option<f64>,
    pub allowances: &'a JsonValue,
    pub loss_carry: &'a JsonValue,
    pub holding_period_rules: &'a JsonValue,
    pub wealth_tax: Option<&'a JsonValue>,
    pub notes: &'a str,
    pub is_custom: bool,
}

pub async fn create_profile(pool: &PgPool, p: &NewProfile<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO taxcalc_profiles (id, name, country, region, currency, person_type, regime, \
         marginal_income_rate, social_charges_rate, allowances, loss_carry, holding_period_rules, \
         wealth_tax, notes, is_custom) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(id)
    .bind(p.name)
    .bind(p.country)
    .bind(p.region)
    .bind(p.currency)
    .bind(p.person_type)
    .bind(p.regime)
    .bind(p.marginal_income_rate)
    .bind(p.social_charges_rate)
    .bind(p.allowances)
    .bind(p.loss_carry)
    .bind(p.holding_period_rules)
    .bind(p.wealth_tax)
    .bind(p.notes)
    .bind(p.is_custom)
    .execute(pool)
    .await
    .context("creating tax profile")?;
    Ok(id)
}

pub async fn update_profile(pool: &PgPool, id: Uuid, p: &NewProfile<'_>) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE taxcalc_profiles SET name=$2, country=$3, region=$4, currency=$5, person_type=$6, \
         regime=$7, marginal_income_rate=$8, social_charges_rate=$9, allowances=$10, \
         loss_carry=$11, holding_period_rules=$12, wealth_tax=$13, notes=$14, is_custom=$15, \
         updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(p.name)
    .bind(p.country)
    .bind(p.region)
    .bind(p.currency)
    .bind(p.person_type)
    .bind(p.regime)
    .bind(p.marginal_income_rate)
    .bind(p.social_charges_rate)
    .bind(p.allowances)
    .bind(p.loss_carry)
    .bind(p.holding_period_rules)
    .bind(p.wealth_tax)
    .bind(p.notes)
    .bind(p.is_custom)
    .execute(pool)
    .await
    .context("updating tax profile")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_profile(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM taxcalc_profiles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ----- Scenarios -----

pub async fn list_scenarios(pool: &PgPool) -> anyhow::Result<Vec<Scenario>> {
    let sql = format!(
        "SELECT {SCENARIO_COLS} FROM taxcalc_scenarios ORDER BY tax_year DESC, created_at DESC"
    );
    Ok(sqlx::query_as::<_, Scenario>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_scenario(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Scenario>> {
    let sql = format!("SELECT {SCENARIO_COLS} FROM taxcalc_scenarios WHERE id = $1");
    Ok(sqlx::query_as::<_, Scenario>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub struct NewScenario<'a> {
    pub profile_id: Uuid,
    pub name: &'a str,
    pub tax_year: i32,
    pub mode: &'a str,
    pub context: &'a str,
    pub currency: &'a str,
    pub inputs: &'a JsonValue,
}

pub async fn create_scenario(pool: &PgPool, s: &NewScenario<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO taxcalc_scenarios (id, profile_id, name, tax_year, mode, context, currency, \
         inputs) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(s.profile_id)
    .bind(s.name)
    .bind(s.tax_year)
    .bind(s.mode)
    .bind(s.context)
    .bind(s.currency)
    .bind(s.inputs)
    .execute(pool)
    .await
    .context("creating tax scenario")?;
    Ok(id)
}

pub async fn update_scenario(pool: &PgPool, id: Uuid, s: &NewScenario<'_>) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE taxcalc_scenarios SET profile_id=$2, name=$3, tax_year=$4, mode=$5, context=$6, \
         currency=$7, inputs=$8, result=NULL, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(s.profile_id)
    .bind(s.name)
    .bind(s.tax_year)
    .bind(s.mode)
    .bind(s.context)
    .bind(s.currency)
    .bind(s.inputs)
    .execute(pool)
    .await
    .context("updating tax scenario")?;
    Ok(res.rows_affected() > 0)
}

/// Cache the engine's computed breakdown on a scenario.
pub async fn save_result(pool: &PgPool, id: Uuid, result: &JsonValue) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE taxcalc_scenarios SET result=$2, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(result)
    .execute(pool)
    .await
    .context("saving tax scenario result")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_scenario(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM taxcalc_scenarios WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
