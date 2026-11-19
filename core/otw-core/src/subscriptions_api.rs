//! HTTP API for the Subscription Tracker module.
//!
//! Subscriptions CRUD, autocomplete suggestions (platforms/categories/payment methods), a
//! monthly-spend breakdown, price history, usage ratings, the 12-month forecast, the iCal
//! export, and the settings (display currency, category budgets). Validation mirrors the
//! journal's currency whitelist so FX conversion stays coherent.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
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
        .route("/api/subscriptions/forecast", get(forecast))
        .route("/api/subscriptions/calendar.ics", get(calendar_ics))
        .route("/api/subscriptions/{id}/prices", get(prices))
        .route("/api/subscriptions/{id}/ratings", post(rate))
}

/// Re-check the category budgets after an edit, off the request: a breach notifies once.
fn spawn_budget_check(state: &AppState) {
    let (pool, cipher, http) = (state.pool.clone(), state.cipher.clone(), state.http.clone());
    tokio::spawn(async move {
        if let Err(e) = crate::subscriptions::check_budgets(&pool, &cipher, &http).await {
            tracing::error!("subscriptions budget check failed: {e:#}");
        }
    });
}

/// Every channel id must exist and be granted to Subscriptions.
async fn validate_channels(state: &AppState, ids: &[Uuid]) -> Result<(), ApiError> {
    for id in ids {
        match otw_store::notif_channels::get(&state.pool, *id).await? {
            Some(ch) if ch.allows("subscriptions") => {}
            Some(_) => {
                return Err(ApiError::bad_request(
                    "this notification channel is not allowed for Subscriptions",
                ))
            }
            None => return Err(ApiError::bad_request("unknown notification channel")),
        }
    }
    Ok(())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ListQuery {
    platform: Option<String>,
    category: Option<String>,
    #[serde(default, deserialize_with = "crate::util::lenient_bool::deserialize_lenient_bool")]
    #[schemars(with = "bool")]
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
    if input.remind_days.is_some_and(|d| !(0..=60).contains(&d)) {
        return Err(ApiError::bad_request("reminder days must be between 0 and 60"));
    }
    for (label, value, parsed) in [
        ("start date", &input.started_on, input.started_date()),
        ("trial end", &input.trial_end, input.trial_date()),
        ("end date", &input.ended_at, input.ended_date()),
    ] {
        if value.as_deref().is_some_and(|v| !v.is_empty()) && parsed.is_none() {
            return Err(ApiError::bad_request(&format!("{label} must be YYYY-MM-DD")));
        }
    }
    if input.yearly_price.is_some_and(|p| !(p.is_finite() && p >= 0.0)) {
        return Err(ApiError::bad_request("yearly price must be a non-negative number"));
    }
    if input.card_expiry.as_deref().is_some_and(|c| !subscriptions::valid_card_expiry(c)) {
        return Err(ApiError::bad_request("card expiry must be YYYY-MM"));
    }
    if !(input.my_share_pct.is_finite() && input.my_share_pct > 0.0 && input.my_share_pct <= 100.0) {
        return Err(ApiError::bad_request("your share must be above 0 and at most 100 %"));
    }
    Ok(())
}

/// Blank optional text reads as none, so a cleared field is stored as null.
fn normalize(input: &mut SubscriptionInput) {
    input.name = input.name.trim().to_string();
    for f in [&mut input.payment_method, &mut input.card_expiry] {
        *f = empty_to_none(f.take().map(|v| v.trim().to_string()));
    }
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<SubscriptionInput>,
) -> Result<Json<Value>, ApiError> {
    normalize(&mut input);
    validate(&input)?;
    validate_channels(&state, &input.remind_channels).await?;
    let sub = subscriptions::add_subscription(&state.pool, &input).await?;
    spawn_budget_check(&state);
    Ok(Json(json!({ "subscription": sub })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<SubscriptionInput>,
) -> Result<Json<Value>, ApiError> {
    normalize(&mut input);
    validate(&input)?;
    validate_channels(&state, &input.remind_channels).await?;
    if !subscriptions::update_subscription(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("subscription not found"));
    }
    spawn_budget_check(&state);
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
    Ok(Json(json!({
        "platforms": s.platforms,
        "categories": s.categories,
        "payment_methods": s.payment_methods,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BreakdownQuery {
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
    /// Monthly cap per category, in the display currency. Replaces the whole map.
    category_budgets: Option<BTreeMap<String, f64>>,
    /// Channel ids for a budget breach; `[]` = every channel granted to Subscriptions.
    budget_channels: Option<Vec<Uuid>>,
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
    let budgets = match body.category_budgets {
        Some(b) => {
            let mut out = BTreeMap::new();
            for (cat, cap) in b {
                let cat = cat.trim().to_string();
                if cat.is_empty() {
                    return Err(ApiError::bad_request("a budget needs a category"));
                }
                if !(cap.is_finite() && cap > 0.0) {
                    return Err(ApiError::bad_request("a budget must be a positive amount"));
                }
                out.insert(cat, cap);
            }
            Some(out)
        }
        None => None,
    };
    if let Some(c) = &body.budget_channels {
        validate_channels(&state, c).await?;
    }
    if budgets.is_some() || body.budget_channels.is_some() {
        subscriptions::set_budgets(&state.pool, budgets.as_ref(), body.budget_channels.as_deref())
            .await?;
        spawn_budget_check(&state);
    }
    let settings = subscriptions::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

async fn prices(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let sub = subscriptions::get_subscription(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("subscription not found"))?;
    let prices = subscriptions::price_history(&state.pool, &sub).await?;
    Ok(Json(json!({ "prices": prices })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RatingBody {
    /// 1 (not worth it) to 5 (essential).
    score: i16,
}

async fn rate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<RatingBody>,
) -> Result<Json<Value>, ApiError> {
    if !(1..=5).contains(&body.score) {
        return Err(ApiError::bad_request("score must be between 1 and 5"));
    }
    if !subscriptions::add_rating(&state.pool, id, body.score).await? {
        return Err(ApiError::not_found("subscription not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// The next 12 calendar months of outflows, per month in the display currency and per
/// billing currency. MyWealth reads it as a recurring liability: keep the shape stable.
async fn forecast(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = subscriptions::get_settings(&state.pool).await?;
    let f = subscriptions::forecast(&state.pool, &settings.display_currency).await?;
    Ok(Json(serde_json::to_value(f).map_err(anyhow::Error::from)?))
}

/// Every billing date of the next 12 months as an all-day event. A download: refused in
/// demo mode.
async fn calendar_ics(State(state): State<AppState>) -> Result<Response, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden_code(
            "demo_disabled",
            "downloads are disabled in the demo",
        ));
    }
    let subs = subscriptions::list_subscriptions(
        &state.pool,
        &SubFilter { active_only: true, ..Default::default() },
    )
    .await?;
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let body = subscriptions::ical_calendar(&subs, now.date(), &stamp);
    Ok((
        [
            (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"subscriptions.ics\""),
        ],
        body,
    )
        .into_response())
}
