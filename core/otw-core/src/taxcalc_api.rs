//! HTTP API for the TaxCalculator module.
//!
//! Templates are read-only static data (the country rule library). Profiles and Scenarios are
//! CRUD. `compute` runs the pure engine against a scenario's profile and caches the breakdown.
//! Validation lives here; the engine is pure. Estimates only — not tax advice.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{taxcalc, ApiError, AppState};
use otw_store::taxcalc as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/taxcalc/templates", get(templates))
        .route("/api/taxcalc/profiles", get(list_profiles).post(create_profile))
        .route(
            "/api/taxcalc/profiles/{id}",
            get(get_profile).put(update_profile).delete(delete_profile),
        )
        .route("/api/taxcalc/compute", post(compute_stateless))
        .route("/api/taxcalc/scenarios", get(list_scenarios).post(create_scenario))
        .route(
            "/api/taxcalc/scenarios/{id}",
            get(get_scenario).put(update_scenario).delete(delete_scenario),
        )
        .route("/api/taxcalc/scenarios/{id}/compute", post(compute))
        .route("/api/taxcalc/profiles/{id}/losses", get(list_losses))
        .route(
            "/api/taxcalc/profiles/{id}/losses/{year}/{pool}",
            axum::routing::put(put_loss).delete(delete_loss),
        )
}

async fn templates() -> Json<Value> {
    Json(json!({ "templates": taxcalc::templates() }))
}

// ----- Profiles -----

async fn list_profiles(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let profiles = store::list_profiles(&state.pool).await?;
    Ok(Json(json!({ "profiles": profiles })))
}

async fn get_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let p = store::get_profile(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("profile not found"))?;
    Ok(Json(json!({ "profile": p })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ProfileBody {
    name: String,
    country: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(default = "default_person")]
    person_type: String,
    #[serde(default = "default_regime")]
    regime: String,
    /// Flat rate on gains (percent). Absent/null = use the regime template's flat rate.
    #[serde(default)]
    flat_rate: Option<f64>,
    #[serde(default)]
    marginal_income_rate: Option<f64>,
    #[serde(default)]
    social_charges_rate: Option<f64>,
    #[serde(default = "empty_obj")]
    allowances: Value,
    #[serde(default = "empty_obj")]
    loss_carry: Value,
    #[serde(default = "empty_arr")]
    holding_period_rules: Value,
    #[serde(default)]
    wealth_tax: Option<Value>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    is_custom: bool,
    /// average | fifo | uk_pool. Absent/null = the regime's method.
    #[serde(default)]
    cost_method: Option<String>,
}

fn default_currency() -> String {
    "USD".into()
}
fn default_person() -> String {
    "individual".into()
}
fn default_regime() -> String {
    "custom_flat".into()
}
fn empty_obj() -> Value {
    json!({})
}
fn empty_arr() -> Value {
    json!([])
}

fn validate_profile(b: &ProfileBody) -> Result<(), ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if b.country.trim().is_empty() {
        return Err(ApiError::bad_request("country is required"));
    }
    if b.person_type != "individual" && b.person_type != "professional" {
        return Err(ApiError::bad_request("person_type must be individual or professional"));
    }
    if let Some(m) = b.cost_method.as_deref().filter(|m| !m.is_empty()) {
        if taxcalc::lots::Method::parse(m).is_none() {
            return Err(ApiError::bad_request("cost_method must be average, fifo or uk_pool"));
        }
    }
    Ok(())
}

fn to_new_profile(b: &ProfileBody) -> store::NewProfile<'_> {
    store::NewProfile {
        name: b.name.trim(),
        country: b.country.trim(),
        region: b.region.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        currency: b.currency.trim(),
        person_type: &b.person_type,
        regime: &b.regime,
        flat_rate: b.flat_rate,
        marginal_income_rate: b.marginal_income_rate,
        social_charges_rate: b.social_charges_rate,
        allowances: &b.allowances,
        loss_carry: &b.loss_carry,
        holding_period_rules: &b.holding_period_rules,
        wealth_tax: b.wealth_tax.as_ref(),
        notes: b.notes.trim(),
        is_custom: b.is_custom,
        cost_method: b.cost_method.as_deref().filter(|m| !m.is_empty()),
    }
}

async fn create_profile(
    State(state): State<AppState>,
    Json(body): Json<ProfileBody>,
) -> Result<Json<Value>, ApiError> {
    validate_profile(&body)?;
    let id = store::create_profile(&state.pool, &to_new_profile(&body)).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ProfileBody>,
) -> Result<Json<Value>, ApiError> {
    validate_profile(&body)?;
    if !store::update_profile(&state.pool, id, &to_new_profile(&body)).await? {
        return Err(ApiError::not_found("profile not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_profile(&state.pool, id).await? {
        return Err(ApiError::not_found("profile not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ----- Scenarios -----

async fn list_scenarios(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let scenarios = store::list_scenarios(&state.pool).await?;
    Ok(Json(json!({ "scenarios": scenarios })))
}

async fn get_scenario(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_scenario(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("scenario not found"))?;
    Ok(Json(json!({ "scenario": s })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ScenarioBody {
    profile_id: Uuid,
    name: String,
    tax_year: i32,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_context")]
    context: String,
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(default = "empty_obj")]
    inputs: Value,
}

fn default_mode() -> String {
    "summary".into()
}
fn default_context() -> String {
    "investing".into()
}

fn validate_scenario(b: &ScenarioBody) -> Result<(), ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if b.mode != "summary" && b.mode != "itemized" {
        return Err(ApiError::bad_request("mode must be summary or itemized"));
    }
    if b.context != "trading" && b.context != "investing" {
        return Err(ApiError::bad_request("context must be trading or investing"));
    }
    Ok(())
}

fn to_new_scenario(b: &ScenarioBody) -> store::NewScenario<'_> {
    store::NewScenario {
        profile_id: b.profile_id,
        name: b.name.trim(),
        tax_year: b.tax_year,
        mode: &b.mode,
        context: &b.context,
        currency: b.currency.trim(),
        inputs: &b.inputs,
    }
}

async fn create_scenario(
    State(state): State<AppState>,
    Json(body): Json<ScenarioBody>,
) -> Result<Json<Value>, ApiError> {
    validate_scenario(&body)?;
    store::get_profile(&state.pool, body.profile_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("profile not found"))?;
    let id = store::create_scenario(&state.pool, &to_new_scenario(&body)).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_scenario(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ScenarioBody>,
) -> Result<Json<Value>, ApiError> {
    validate_scenario(&body)?;
    if !store::update_scenario(&state.pool, id, &to_new_scenario(&body)).await? {
        return Err(ApiError::not_found("scenario not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_scenario(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_scenario(&state.pool, id).await? {
        return Err(ApiError::not_found("scenario not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Run the engine against a profile + scenario inputs WITHOUT persisting anything. This is
/// the "Calculate" path: results are ephemeral. Saving to history is a separate explicit
/// action (create_scenario). Only the referenced profile must exist.
async fn compute_stateless(
    State(state): State<AppState>,
    Json(body): Json<ScenarioBody>,
) -> Result<Json<Value>, ApiError> {
    validate_scenario(&body)?;
    let profile = store::get_profile(&state.pool, body.profile_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("profile not found"))?;

    let profile_json = serde_json::to_value(&profile).map_err(|e| anyhow::anyhow!(e))?;
    let mut inputs = body.inputs.clone();
    with_registry(&state, &profile_json, body.profile_id, body.tax_year, &mut inputs).await?;
    // Shape a scenario-like JSON the engine understands (it only reads mode/context/inputs).
    let scenario_json = json!({
        "tax_year": body.tax_year,
        "mode": body.mode,
        "context": body.context,
        "currency": body.currency,
        "inputs": inputs,
    });
    let result = taxcalc::compute(&profile_json, &scenario_json);
    Ok(Json(json!({ "result": result })))
}

/// Run the engine for a scenario and cache the breakdown on the row.
async fn compute(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let scenario = store::get_scenario(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("scenario not found"))?;
    let profile = store::get_profile(&state.pool, scenario.profile_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("scenario's profile no longer exists"))?;

    let profile_json = serde_json::to_value(&profile).map_err(|e| anyhow::anyhow!(e))?;
    let mut scenario_json = serde_json::to_value(&scenario).map_err(|e| anyhow::anyhow!(e))?;
    let mut inputs = scenario.inputs.clone();
    with_registry(&state, &profile_json, scenario.profile_id, scenario.tax_year, &mut inputs).await?;
    scenario_json["inputs"] = inputs;
    let result = taxcalc::compute(&profile_json, &scenario_json);

    store::save_result(&state.pool, id, &result).await?;
    Ok(Json(json!({ "result": result })))
}

// ----- Loss registry -----

/// Put the registry's carried losses for `year` into the engine's inputs, per pool. A
/// profile with no registry rows keeps the hand-typed `prior_losses_carried` figure.
async fn with_registry(
    state: &AppState,
    profile: &Value,
    profile_id: Uuid,
    year: i32,
    inputs: &mut Value,
) -> Result<(), ApiError> {
    let rows = store::list_loss_years(&state.pool, profile_id).await?;
    if rows.is_empty() || !inputs.is_object() {
        return Ok(());
    }
    let rows: Vec<(i32, String, f64)> = rows.into_iter().map(|r| (r.tax_year, r.pool, r.net)).collect();
    let carry = taxcalc::carry_into(&rows, &taxcalc::pool_rules(profile, year), year);
    let map: serde_json::Map<String, Value> =
        carry.into_iter().map(|c| (c.pool, json!(c.available))).collect();
    inputs["carried_losses"] = Value::Object(map);
    Ok(())
}

#[derive(Deserialize)]
struct LossQuery {
    year: Option<i32>,
}

/// The registry rows, the profile's pools, and what each pool carries into `year`
/// (default: the current year).
async fn list_losses(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<LossQuery>,
) -> Result<Json<Value>, ApiError> {
    let profile = store::get_profile(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("profile not found"))?;
    let profile_json = serde_json::to_value(&profile).map_err(|e| anyhow::anyhow!(e))?;
    let rows = store::list_loss_years(&state.pool, id).await?;
    let year = q.year.unwrap_or_else(|| time::OffsetDateTime::now_utc().year());
    let rules = taxcalc::pool_rules(&profile_json, year);
    let flat: Vec<(i32, String, f64)> = rows.iter().map(|r| (r.tax_year, r.pool.clone(), r.net)).collect();
    let carry = taxcalc::carry_into(&flat, &rules, year);
    Ok(Json(json!({ "year": year, "pools": rules, "rows": rows, "carry": carry })))
}

#[derive(Deserialize)]
struct LossBody {
    net: f64,
    #[serde(default)]
    note: String,
}

async fn put_loss(
    State(state): State<AppState>,
    Path((id, year, pool)): Path<(Uuid, i32, String)>,
    Json(body): Json<LossBody>,
) -> Result<Json<Value>, ApiError> {
    let profile = store::get_profile(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("profile not found"))?;
    let profile_json = serde_json::to_value(&profile).map_err(|e| anyhow::anyhow!(e))?;
    if !taxcalc::pool_rules(&profile_json, year).iter().any(|r| r.key == pool) {
        return Err(ApiError::bad_request(&format!(
            "{pool} is not a pool of this profile's regime"
        )));
    }
    if !(1900..=2200).contains(&year) {
        return Err(ApiError::bad_request("tax year out of range"));
    }
    if !body.net.is_finite() {
        return Err(ApiError::bad_request("net must be a number"));
    }
    store::upsert_loss_year(&state.pool, id, year, &pool, body.net, body.note.trim()).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_loss(
    State(state): State<AppState>,
    Path((id, year, pool)): Path<(Uuid, i32, String)>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_loss_year(&state.pool, id, year, &pool).await? {
        return Err(ApiError::not_found("no such registry row"));
    }
    Ok(Json(json!({ "ok": true })))
}
