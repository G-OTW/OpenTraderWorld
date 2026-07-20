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
        // Calendar — daily PnL heatmap (read-only aggregation)
        .route("/api/journal/calendar", get(calendar))
        // Export: raw trades as CSV + periodic performance report (Markdown/PDF)
        .route("/api/journal/export/trades.csv", get(export_trades_csv))
        .route("/api/journal/report", get(periodic_report))
}

// ── Categories ───────────────────────────────────────────────────────────────

async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let categories = journal::list_categories(&state.pool).await?;
    Ok(Json(json!({ "categories": categories })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CategoryBody {
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CapitalBody {
    #[serde(default = "default_refill")]
    kind: String,
    #[serde(default)]
    amount: f64,
    #[serde(default = "default_usd")]
    currency: String,
    note: Option<String>,
    /// RFC3339 timestamp; defaults to now.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schemars(with = "Option<String>")]
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StrategyBody {
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TemplateBody {
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SettingsBody {
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

// ── Export: trades CSV ───────────────────────────────────────────────────────

/// Stream the filtered trade list as CSV (native currencies, computed PnL
/// included; advanced trades carry their legs as compact JSON columns).
async fn export_trades_csv(
    State(state): State<AppState>,
    Query(q): Query<TradeQuery>,
) -> Result<axum::response::Response, ApiError> {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let filter = q.into_filter()?;
    let mut trades = journal::list_trades(&state.pool, &filter).await?;
    // Chronological order reads better in a spreadsheet than the UI's newest-first.
    trades.sort_by_key(|t| t.exit_at.or(t.entry_at).unwrap_or(t.created_at));
    let strategies = journal::strategy_names(&state.pool).await?;
    let categories = journal::category_names(&state.pool).await?;

    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record([
        "id", "date", "status", "ticker", "asset_class", "exchange", "side", "quantity",
        "unit_type", "avg_entry", "avg_exit", "entry_at", "exit_at", "currency", "fees",
        "leverage", "multiplier", "gross_pnl", "net_pnl", "open_qty", "category", "strategy",
        "signal", "feedback", "advanced", "cost_basis_method", "entries", "exits",
    ])
    .map_err(csv_err)?;

    let dt = |v: &Option<OffsetDateTime>| {
        v.and_then(|d| d.format(&Rfc3339).ok()).unwrap_or_default()
    };
    let num = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
    for t in &trades {
        let effective = t.exit_at.or(t.entry_at).unwrap_or(t.created_at);
        let legs = |v: &JsonValue| {
            if t.advanced { serde_json::to_string(v).unwrap_or_default() } else { String::new() }
        };
        w.write_record([
            t.id.to_string(),
            effective.format(&Rfc3339).unwrap_or_default(),
            if t.net_pnl.is_some() { "closed".into() } else { "open".into() },
            t.ticker.clone(),
            t.asset_class.clone(),
            t.exchange.clone().unwrap_or_default(),
            t.side.clone(),
            fmt_qty(journal::trade_entry_qty(t)),
            t.unit_type.clone(),
            num(t.avg_entry),
            num(journal::trade_avg_exit(t)),
            dt(&t.entry_at),
            dt(&t.exit_at),
            t.currency.clone(),
            t.fees.to_string(),
            t.leverage.to_string(),
            t.multiplier.to_string(),
            num(t.gross_pnl),
            num(t.net_pnl),
            fmt_qty(t.open_qty),
            categories.get(&t.category_id).cloned().unwrap_or_default(),
            t.strategy_id
                .and_then(|id| strategies.get(&id).cloned())
                .unwrap_or_default(),
            t.signal_name.clone().unwrap_or_default(),
            t.feedback.clone().unwrap_or_default(),
            t.advanced.to_string(),
            t.cost_basis_method.clone(),
            legs(&t.entries),
            legs(&t.exits),
        ])
        .map_err(csv_err)?;
    }
    let bytes = w.into_inner().map_err(|_| ApiError::internal("csv buffer"))?;

    let today = OffsetDateTime::now_utc().date();
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"journal_trades_{today}.csv\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn csv_err(e: csv::Error) -> ApiError {
    tracing::error!("csv export failed: {e}");
    ApiError::internal("csv encoding failed")
}

/// Quantity without float noise, empty when zero-ish inputs were missing.
fn fmt_qty(q: f64) -> String {
    journal::round_dp(q, 8).to_string()
}

// ── Export: periodic report (Markdown / PDF) ─────────────────────────────────

#[derive(Deserialize)]
struct ReportQuery {
    /// "week" (ISO Monday-based) or "month".
    #[serde(default = "default_period")]
    period: String,
    /// Any date inside the wanted period (YYYY-MM-DD, default: today UTC).
    anchor: Option<String>,
    /// "md" or "pdf".
    #[serde(default = "default_format")]
    format: String,
    // Same filters as the trades list / breakdown.
    category_id: Option<Uuid>,
    strategy_id: Option<Uuid>,
    asset_class: Option<String>,
    side: Option<String>,
    ticker: Option<String>,
    signal_name: Option<String>,
}
fn default_period() -> String {
    "week".to_string()
}
fn default_format() -> String {
    "md".to_string()
}

/// [start, end) of the report period plus a human label and a filename slug.
fn period_bounds(
    period: &str,
    anchor: time::Date,
) -> Result<(OffsetDateTime, OffsetDateTime, String, String), ApiError> {
    use time::macros::format_description;
    let day = format_description!("[month repr:short] [day padding:none]");
    let day_year = format_description!("[month repr:short] [day padding:none], [year]");
    match period {
        "week" => {
            let monday = anchor - time::Duration::days(anchor.weekday().number_days_from_monday() as i64);
            let sunday = monday + time::Duration::days(6);
            let start = monday.midnight().assume_utc();
            let end = start + time::Duration::days(7);
            let (iso_year, iso_week, _) = monday.to_iso_week_date();
            let label = format!(
                "{} – {} (week {iso_week})",
                monday.format(day).unwrap_or_default(),
                sunday.format(day_year).unwrap_or_default(),
            );
            Ok((start, end, label, format!("{iso_year}-W{iso_week:02}")))
        }
        "month" => {
            let first = anchor.replace_day(1).map_err(|_| ApiError::bad_request("bad anchor date"))?;
            let (ny, nm) = match u8::from(first.month()) {
                12 => (first.year() + 1, time::Month::January),
                m => (first.year(), time::Month::try_from(m + 1).unwrap()),
            };
            let next = time::Date::from_calendar_date(ny, nm, 1)
                .map_err(|_| ApiError::bad_request("bad anchor date"))?;
            let month_year = format_description!("[month repr:long] [year]");
            let label = first.format(month_year).unwrap_or_default();
            let slug = format!("{}-{:02}", first.year(), u8::from(first.month()));
            Ok((first.midnight().assume_utc(), next.midnight().assume_utc(), label, slug))
        }
        _ => Err(ApiError::bad_request("period must be week or month")),
    }
}

/// Weekly/monthly performance report over the filtered trades, rendered by the
/// shared report engine to Markdown or PDF.
async fn periodic_report(
    State(state): State<AppState>,
    Query(q): Query<ReportQuery>,
) -> Result<axum::response::Response, ApiError> {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    use time::macros::format_description;

    let anchor = match &q.anchor {
        Some(s) if !s.is_empty() => {
            time::Date::parse(s, format_description!("[year]-[month]-[day]"))
                .map_err(|_| ApiError::bad_request("anchor must be YYYY-MM-DD"))?
        }
        _ => OffsetDateTime::now_utc().date(),
    };
    let (start, end, period_label, slug) = period_bounds(&q.period, anchor)?;

    let filter = TradeFilter {
        category_id: q.category_id,
        strategy_id: q.strategy_id,
        asset_class: empty_to_none(q.asset_class),
        side: empty_to_none(q.side),
        ticker: empty_to_none(q.ticker),
        signal_name: empty_to_none(q.signal_name),
        since: Some(start),
        // The shared filter is inclusive on both ends; step just inside the period.
        until: Some(end - time::Duration::microseconds(1)),
    };

    let settings = journal::get_settings(&state.pool).await?;
    let data = journal::report_data(&state.pool, &filter, &settings.display_currency).await?;

    // Scope + filter description for the report header.
    let categories = journal::category_names(&state.pool).await?;
    let strategies = journal::strategy_names(&state.pool).await?;
    let scope = match filter.category_id {
        Some(id) => categories.get(&id).cloned().unwrap_or_else(|| "Unknown category".into()),
        None => "All categories".to_string(),
    };
    let mut filters: Vec<(String, String)> = Vec::new();
    if let Some(id) = filter.strategy_id {
        if let Some(n) = strategies.get(&id) {
            filters.push(("strategy".into(), n.clone()));
        }
    }
    for (k, v) in [
        ("asset_class", &filter.asset_class),
        ("side", &filter.side),
        ("ticker", &filter.ticker),
        ("signal", &filter.signal_name),
    ] {
        if let Some(v) = v {
            filters.push((k.into(), v.clone()));
        }
    }

    let ctx = crate::journal_report::ReportContext {
        title: if q.period == "month" {
            "Monthly trading report".into()
        } else {
            "Weekly trading report".into()
        },
        period_label,
        period_slug: slug.clone(),
        scope,
        filters,
        generated_at: OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .unwrap_or_else(|_| OffsetDateTime::now_utc())
            .format(&Rfc3339)
            .unwrap_or_default(),
    };
    let doc = crate::journal_report::build(&data, &ctx);

    let (body, mime, ext): (Vec<u8>, &str, &str) = match q.format.as_str() {
        "pdf" => (crate::report::pdf::render(&doc), "application/pdf", "pdf"),
        "md" => (
            crate::report::markdown::render(&doc).into_bytes(),
            "text/markdown; charset=utf-8",
            "md",
        ),
        _ => return Err(ApiError::bad_request("format must be md or pdf")),
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"journal_report_{slug}.{ext}\""),
            ),
        ],
        body,
    )
        .into_response())
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

// ── Calendar ─────────────────────────────────────────────────────────────────

/// Extra query param for the calendar: the viewer's UTC offset in minutes, exactly
/// as JS `getTimezoneOffset()` reports it (minutes to add to local to get UTC).
/// Defaults to 0 (UTC) when absent; clamped to a sane range.
#[derive(Deserialize)]
struct CalendarQuery {
    #[serde(default)]
    tz_offset: i32,
}

/// Daily realized-PnL buckets over the filtered trades (same filter set as the
/// breakdown), in the journal's display currency. Powers the month-grid heatmap.
async fn calendar(
    State(state): State<AppState>,
    Query(q): Query<TradeQuery>,
    Query(cal): Query<CalendarQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = q.into_filter()?;
    // Guard against absurd offsets (valid range is roughly ±14h = ±840 min).
    let tz_offset = cal.tz_offset.clamp(-840, 840);
    let settings = journal::get_settings(&state.pool).await?;
    let days =
        journal::calendar(&state.pool, &filter, &settings.display_currency, tz_offset).await?;
    Ok(Json(json!({ "days": days, "display_currency": settings.display_currency })))
}
