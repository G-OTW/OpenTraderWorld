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

#[derive(Deserialize)]
struct ProfileBody {
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
        marginal_income_rate: b.marginal_income_rate,
        social_charges_rate: b.social_charges_rate,
        allowances: &b.allowances,
        loss_carry: &b.loss_carry,
        holding_period_rules: &b.holding_period_rules,
        wealth_tax: b.wealth_tax.as_ref(),
        notes: b.notes.trim(),
        is_custom: b.is_custom,
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

#[derive(Deserialize)]
struct ScenarioBody {
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
    // Shape a scenario-like JSON the engine understands (it only reads mode/context/inputs).
    let scenario_json = json!({
        "mode": body.mode,
        "context": body.context,
        "currency": body.currency,
        "inputs": body.inputs,
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
    let scenario_json = serde_json::to_value(&scenario).map_err(|e| anyhow::anyhow!(e))?;
    let result = taxcalc::compute(&profile_json, &scenario_json);

    store::save_result(&state.pool, id, &result).await?;
    Ok(Json(json!({ "result": result })))
}
