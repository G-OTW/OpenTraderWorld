//! HTTP API for the Trading Journal module.
//!
//! Categories (folders), capital events (beginning stack + refills), strategies,
//! templates (trade-logging forms), trades (typed reserved fields + custom fields),
//! and the performance breakdown (equity curve + stats) per category.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::JsonValue;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::journal::{
    self, CategoryPatch, FeeScheduleInput, FeeSchedulePatch, StrategyPatch, TemplatePatch,
    TradeFilter, TradeInput,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        // Categories
        .route("/api/journal/categories", get(list_categories).post(add_category))
        .route(
            "/api/journal/categories/{id}",
            patch(update_category).delete(delete_category),
        )
        // Capital events
        .route(
            "/api/journal/categories/{id}/capital",
            get(list_capital).post(add_capital),
        )
        .route("/api/journal/capital/{id}", delete(delete_capital))
        // Strategies
        .route("/api/journal/strategies", get(list_strategies).post(add_strategy))
        .route(
            "/api/journal/strategies/{id}",
            patch(update_strategy).delete(delete_strategy),
        )
        // Templates
        .route("/api/journal/templates", get(list_templates).post(add_template))
        .route(
            "/api/journal/templates/{id}",
            patch(update_template).delete(delete_template),
        )
        // Fee schedules
        .route(
            "/api/journal/fee-schedules",
            get(list_fee_schedules).post(add_fee_schedule),
        )
        .route(
            "/api/journal/fee-schedules/{id}",
            patch(update_fee_schedule).delete(delete_fee_schedule),
        )
        // Settings
        .route("/api/journal/settings", get(get_settings).patch(update_settings))
        // Trades
        .route("/api/journal/trades", get(list_trades).post(add_trade))
        .route(
            "/api/journal/trades/{id}",
            get(get_trade).patch(update_trade).delete(delete_trade),
        )
        // Autocomplete
        .route("/api/journal/trade-suggestions", get(trade_suggestions))
        // Breakdown
        .route("/api/journal/breakdown", get(breakdown))
}

// ── Categories ───────────────────────────────────────────────────────────────

async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let categories = journal::list_categories(&state.pool).await?;
    Ok(Json(json!({ "categories": categories })))
}

#[derive(Deserialize)]
struct CategoryBody {
    #[serde(default)]
    name: String,
    color: Option<String>,
}

async fn add_category(
    State(state): State<AppState>,
    Json(body): Json<CategoryBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("category name required"));
    }
    let cat = journal::add_category(&state.pool, name, body.color.as_deref()).await?;
    Ok(Json(json!({ "category": cat })))
}

async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<CategoryPatch>,
) -> Result<Json<Value>, ApiError> {
    if !journal::update_category(&state.pool, id, &patch).await? {
        return Err(ApiError::not_found("category not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_category(&state.pool, id).await? {
        return Err(ApiError::bad_request(
            "category not found or is the protected default",
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Capital events ───────────────────────────────────────────────────────────

async fn list_capital(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let events = journal::list_capital_events(&state.pool, id).await?;
    Ok(Json(json!({ "events": events })))
}

#[derive(Deserialize)]
struct CapitalBody {
    #[serde(default = "default_refill")]
    kind: String,
    #[serde(default)]
    amount: f64,
    #[serde(default = "default_usd")]
    currency: String,
    note: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    occurred_at: Option<OffsetDateTime>,
}
fn default_refill() -> String {
    "refill".to_string()
}
fn default_usd() -> String {
    "USD".to_string()
}

async fn add_capital(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CapitalBody>,
) -> Result<Json<Value>, ApiError> {
    if !journal::CAPITAL_KINDS.contains(&body.kind.as_str()) {
        return Err(ApiError::bad_request("unknown capital event kind"));
    }
    if !journal::CURRENCIES.contains(&body.currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    let event = journal::add_capital_event(
        &state.pool,
        id,
        &body.kind,
        body.amount,
        &body.currency,
        body.note.as_deref(),
        body.occurred_at,
    )
    .await?;
    Ok(Json(json!({ "event": event })))
}

async fn delete_capital(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_capital_event(&state.pool, id).await? {
        return Err(ApiError::not_found("capital event not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Strategies ───────────────────────────────────────────────────────────────

async fn list_strategies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let strategies = journal::list_strategies(&state.pool).await?;
    Ok(Json(json!({ "strategies": strategies })))
}

#[derive(Deserialize)]
struct StrategyBody {
    #[serde(default)]
    name: String,
    description: Option<String>,
    #[serde(default = "empty_array")]
    signals: JsonValue,
}
fn empty_array() -> JsonValue {
    JsonValue::Array(vec![])
}

async fn add_strategy(
    State(state): State<AppState>,
    Json(body): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("strategy name required"));
    }
    let strategy = journal::add_strategy(
        &state.pool,
        body.name.trim(),
        body.description.as_deref(),
        &body.signals,
    )
    .await?;
    Ok(Json(json!({ "strategy": strategy })))
}

async fn update_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<StrategyPatch>,
) -> Result<Json<Value>, ApiError> {
    if !journal::update_strategy(&state.pool, id, &patch).await? {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_strategy(&state.pool, id).await? {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Templates ────────────────────────────────────────────────────────────────

async fn list_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let templates = journal::list_templates(&state.pool).await?;
    Ok(Json(json!({ "templates": templates })))
}

#[derive(Deserialize)]
struct TemplateBody {
    #[serde(default)]
    name: String,
    description: Option<String>,
    #[serde(default = "empty_array")]
    fields: JsonValue,
    default_fee_schedule_id: Option<Uuid>,
}

async fn add_template(
    State(state): State<AppState>,
    Json(body): Json<TemplateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = if body.name.trim().is_empty() {
        "Untitled template"
    } else {
        body.name.trim()
    };
    let template = journal::add_template(
        &state.pool,
        name,
        body.description.as_deref(),
        &body.fields,
        body.default_fee_schedule_id,
    )
    .await?;
    Ok(Json(json!({ "template": template })))
}

async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<TemplatePatch>,
) -> Result<Json<Value>, ApiError> {
    if !journal::update_template(&state.pool, id, &patch).await? {
        return Err(ApiError::bad_request(
            "template not found or is a protected built-in",
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_template(&state.pool, id).await? {
        return Err(ApiError::bad_request(
            "template not found or is a protected built-in",
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Fee schedules ────────────────────────────────────────────────────────────

async fn list_fee_schedules(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let schedules = journal::list_fee_schedules(&state.pool).await?;
    Ok(Json(json!({ "fee_schedules": schedules })))
}

fn validate_fee_schedule(name: &str, amount_kind: &str, per: &str, currency: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("fee schedule name required"));
    }
    if !journal::FEE_AMOUNT_KINDS.contains(&amount_kind) {
        return Err(ApiError::bad_request("amount_kind must be fixed or pct"));
    }
    if !journal::FEE_PER.contains(&per) {
        return Err(ApiError::bad_request("per must be lot, unit, contract or trade"));
    }
    if !journal::CURRENCIES.contains(&currency) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    Ok(())
}

async fn add_fee_schedule(
    State(state): State<AppState>,
    Json(mut input): Json<FeeScheduleInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate_fee_schedule(&input.name, &input.amount_kind, &input.per, &input.currency)?;
    let schedule = journal::add_fee_schedule(&state.pool, &input).await?;
    Ok(Json(json!({ "fee_schedule": schedule })))
}

async fn update_fee_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<FeeSchedulePatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(k) = &patch.amount_kind {
        if !journal::FEE_AMOUNT_KINDS.contains(&k.as_str()) {
            return Err(ApiError::bad_request("amount_kind must be fixed or pct"));
        }
    }
    if let Some(p) = &patch.per {
        if !journal::FEE_PER.contains(&p.as_str()) {
            return Err(ApiError::bad_request("per must be lot, unit, contract or trade"));
        }
    }
    if let Some(c) = &patch.currency {
        if !journal::CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
    }
    if !journal::update_fee_schedule(&state.pool, id, &patch).await? {
        return Err(ApiError::not_found("fee schedule not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_fee_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_fee_schedule(&state.pool, id).await? {
        return Err(ApiError::not_found("fee schedule not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Settings ─────────────────────────────────────────────────────────────────

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = journal::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

#[derive(Deserialize)]
struct SettingsBody {
    display_currency: Option<String>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = body.display_currency {
        if !journal::CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
        journal::set_display_currency(&state.pool, &c).await?;
    }
    let settings = journal::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

// ── Trades ───────────────────────────────────────────────────────────────────

/// Shared filter query for both the trades list and the breakdown. Empty strings are
/// treated as "no filter"; `since`/`until` are RFC3339 on the trade's effective date.
#[derive(Deserialize)]
struct TradeQuery {
    category_id: Option<Uuid>,
    strategy_id: Option<Uuid>,
    asset_class: Option<String>,
    side: Option<String>,
    ticker: Option<String>,
    signal_name: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

fn empty_to_none(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

fn parse_dt(s: &Option<String>) -> Result<Option<OffsetDateTime>, ApiError> {
    match s {
        Some(v) if !v.is_empty() => OffsetDateTime::parse(v, &Rfc3339)
            .map(Some)
            .map_err(|_| ApiError::bad_request("since/until must be RFC3339")),
        _ => Ok(None),
    }
}

impl TradeQuery {
    fn into_filter(self) -> Result<TradeFilter, ApiError> {
        Ok(TradeFilter {
            category_id: self.category_id,
            strategy_id: self.strategy_id,
            asset_class: empty_to_none(self.asset_class),
            side: empty_to_none(self.side),
            ticker: empty_to_none(self.ticker),
            signal_name: empty_to_none(self.signal_name),
            since: parse_dt(&self.since)?,
            until: parse_dt(&self.until)?,
        })
    }
}

async fn list_trades(
    State(state): State<AppState>,
    Query(q): Query<TradeQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = q.into_filter()?;
    let trades = journal::list_trades(&state.pool, &filter).await?;
    Ok(Json(json!({ "trades": trades })))
}

async fn get_trade(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let trade = journal::get_trade(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("trade not found"))?;
    Ok(Json(json!({ "trade": trade })))
}

fn validate_trade(input: &TradeInput) -> Result<(), ApiError> {
    if !journal::ASSET_CLASSES.contains(&input.asset_class.as_str()) {
        return Err(ApiError::bad_request("unknown asset class"));
    }
    if input.side != "long" && input.side != "short" {
        return Err(ApiError::bad_request("side must be long or short"));
    }
    if !journal::CURRENCIES.contains(&input.currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    if !journal::UNIT_TYPES.contains(&input.unit_type.as_str()) {
        return Err(ApiError::bad_request("unit_type must be lot, unit, contract or share"));
    }
    if input.images.as_array().map(|a| a.len()).unwrap_or(0) > 2 {
        return Err(ApiError::bad_request("at most 2 images per trade"));
    }
    if input.cost_basis_method != "avg" && input.cost_basis_method != "fifo" {
        return Err(ApiError::bad_request("cost_basis_method must be avg or fifo"));
    }
    if input.advanced {
        // Every leg needs a positive quantity to be meaningful in PnL.
        if input.entries.iter().any(|l| l.qty <= 0.0)
            || input.exits.iter().any(|l| l.qty <= 0.0)
        {
            return Err(ApiError::bad_request("each entry/exit leg needs a positive quantity"));
        }
        for b in &input.brackets {
            if b.kind != "sl" && b.kind != "tp" {
                return Err(ApiError::bad_request("bracket kind must be sl or tp"));
            }
        }
        // Can't close more than was opened (after folding triggered brackets).
        let entry_qty: f64 = input.entries.iter().map(|l| l.qty).sum();
        let exit_qty: f64 = input.exits.iter().map(|l| l.qty).sum();
        if exit_qty > entry_qty + 1e-9 {
            return Err(ApiError::bad_request(
                "total exit quantity exceeds total entry quantity",
            ));
        }
    }
    Ok(())
}

/// Load the trade's selected fee schedule (if any) so `normalize` can auto-fee
/// triggered SL/TP brackets.
async fn trade_fee_schedule(
    state: &AppState,
    input: &TradeInput,
) -> Result<Option<otw_store::journal::FeeSchedule>, ApiError> {
    match input.fee_schedule_id {
        Some(id) => Ok(journal::get_fee_schedule(&state.pool, id).await?),
        None => Ok(None),
    }
}

async fn add_trade(
    State(state): State<AppState>,
    Json(mut input): Json<TradeInput>,
) -> Result<Json<Value>, ApiError> {
    let schedule = trade_fee_schedule(&state, &input).await?;
    input.normalize(schedule.as_ref()); // fold triggered brackets into exit legs
    validate_trade(&input)?;
    let trade = journal::add_trade(&state.pool, &input).await?;
    Ok(Json(json!({ "trade": trade })))
}

async fn update_trade(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<TradeInput>,
) -> Result<Json<Value>, ApiError> {
    let schedule = trade_fee_schedule(&state, &input).await?;
    input.normalize(schedule.as_ref());
    validate_trade(&input)?;
    if !journal::update_trade(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("trade not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_trade(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !journal::delete_trade(&state.pool, id).await? {
        return Err(ApiError::not_found("trade not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Autocomplete ─────────────────────────────────────────────────────────────

async fn trade_suggestions(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let s = journal::trade_suggestions(&state.pool).await?;
    Ok(Json(json!({
        "tickers": s.tickers,
        "exchanges": s.exchanges,
        "signals": s.signals
    })))
}

// ── Breakdown ────────────────────────────────────────────────────────────────

/// Breakdown accepts the same filter set as the trades list (category, strategy, asset
/// class, side, ticker, signal, date range). Invested capital stays category-scoped.
async fn breakdown(
    State(state): State<AppState>,
    Query(q): Query<TradeQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = q.into_filter()?;
    let settings = journal::get_settings(&state.pool).await?;
    let data = journal::breakdown(&state.pool, &filter, &settings.display_currency).await?;
    Ok(Json(json!({ "breakdown": data })))
}
