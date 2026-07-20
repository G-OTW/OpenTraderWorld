//! HTTP API for the Subscription Tracker module.
//!
//! Subscriptions CRUD, autocomplete suggestions (platforms/categories), a monthly-spend
//! breakdown, and the display-currency setting. Validation mirrors the journal's currency
//! whitelist so FX conversion stays coherent.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::subscriptions::{self, SubFilter, SubscriptionInput, FREQUENCIES};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/subscriptions", get(list).post(add))
        .route("/api/subscriptions/{id}", get(get_one).patch(update).delete(remove))
        .route("/api/subscriptions/suggestions", get(suggestions))
        .route("/api/subscriptions/breakdown", get(breakdown))
        .route("/api/subscriptions/settings", get(get_settings).patch(update_settings))
}

#[derive(Deserialize)]
struct ListQuery {
    platform: Option<String>,
    category: Option<String>,
    #[serde(default)]
    active_only: bool,
}

fn empty_to_none(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

impl ListQuery {
    fn into_filter(self) -> SubFilter {
        SubFilter {
            platform: empty_to_none(self.platform),
            category: empty_to_none(self.category),
            active_only: self.active_only,
        }
    }
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let subs = subscriptions::list_subscriptions(&state.pool, &q.into_filter()).await?;
    Ok(Json(json!({ "subscriptions": subs })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let sub = subscriptions::get_subscription(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("subscription not found"))?;
    Ok(Json(json!({ "subscription": sub })))
}

fn validate(input: &SubscriptionInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("subscription name required"));
    }
    if !CURRENCIES.contains(&input.currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    if !FREQUENCIES.contains(&input.frequency.as_str()) {
        return Err(ApiError::bad_request(
            "frequency must be weekly, monthly, quarterly or yearly",
        ));
    }
    if !(input.price.is_finite() && input.price >= 0.0) {
        return Err(ApiError::bad_request("price must be a non-negative number"));
    }
    Ok(())
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<SubscriptionInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    let sub = subscriptions::add_subscription(&state.pool, &input).await?;
    Ok(Json(json!({ "subscription": sub })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<SubscriptionInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    if !subscriptions::update_subscription(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("subscription not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !subscriptions::delete_subscription(&state.pool, id).await? {
        return Err(ApiError::not_found("subscription not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn suggestions(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let s = subscriptions::suggestions(&state.pool).await?;
    Ok(Json(json!({ "platforms": s.platforms, "categories": s.categories })))
}

#[derive(Deserialize)]
struct BreakdownQuery {
    platform: Option<String>,
    category: Option<String>,
    #[serde(default = "default_back")]
    months_back: i64,
    #[serde(default = "default_fwd")]
    months_fwd: i64,
}
fn default_back() -> i64 {
    5
}
fn default_fwd() -> i64 {
    6
}

async fn breakdown(
    State(state): State<AppState>,
    Query(q): Query<BreakdownQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = SubFilter {
        platform: empty_to_none(q.platform),
        category: empty_to_none(q.category),
        active_only: true,
    };
    let settings = subscriptions::get_settings(&state.pool).await?;
    let months_back = q.months_back.clamp(0, 24);
    let months_fwd = q.months_fwd.clamp(0, 24);
    let data = subscriptions::breakdown(
        &state.pool,
        &filter,
        &settings.display_currency,
        months_back,
        months_fwd,
    )
    .await?;
    Ok(Json(json!({ "breakdown": data })))
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = subscriptions::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SettingsBody {
    display_currency: Option<String>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = body.display_currency {
        if !CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
        subscriptions::set_display_currency(&state.pool, &c).await?;
    }
    let settings = subscriptions::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}
