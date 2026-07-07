//! HTTP API for the Portfolio Tracker module.
//!
//! Portfolios → assets (resolved provider symbols) → operations ledger. The listing endpoint
//! returns per-portfolio totals (market value, cost, PnL) in each portfolio's display currency;
//! the detail endpoint returns its assets (with derived position/PnL), the filterable operations
//! list, and the valuation snapshots for the chart. Symbol search resolves crypto via CoinGecko
//! and stocks/ETFs via Yahoo. Refresh re-prices (USD) and snapshots. Single-user: no owner scoping.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::portfolios::prices;
use crate::{ApiError, AppState};
use otw_store::portfolios as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/portfolios", get(list).post(create))
        .route("/api/portfolios/search", get(search))
        .route("/api/portfolios/{id}", get(detail).patch(update).delete(remove))
        .route("/api/portfolios/{id}/refresh", post(refresh))
        .route("/api/portfolios/{id}/reconcile", post(reconcile))
        .route("/api/portfolios/{id}/assets", post(add_asset))
        .route(
            "/api/portfolios/assets/{asset_id}",
            get(asset_detail).patch(patch_asset).delete(delete_asset),
        )
        .route("/api/portfolios/assets/{asset_id}/operations", post(add_operation))
        .route("/api/portfolios/operations/{op_id}", axum::routing::delete(delete_operation))
}

fn parse_date(s: Option<&str>) -> Result<Date, ApiError> {
    match s {
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
        None => Ok(time::OffsetDateTime::now_utc().date()),
    }
}

// ── Portfolios ────────────────────────────────────────────────────────────────

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let portfolios = store::list_portfolios(&state.pool).await?;
    let mut out = Vec::with_capacity(portfolios.len());
    for pf in &portfolios {
        out.push(store::summary(&state.pool, pf).await?);
    }
    Ok(Json(json!({ "portfolios": out })))
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "usd")]
    currency: String,
}
fn usd() -> String {
    "USD".into()
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let pf = store::create_portfolio(&state.pool, name, b.description.trim(), b.currency.trim()).await?;
    Ok(Json(json!({ "portfolio": pf })))
}

async fn detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let assets = store::asset_views(&state.pool, &pf).await?;
    let ops = store::list_portfolio_operations(&state.pool, id).await?;
    let operations: Vec<Value> = ops
        .into_iter()
        .map(|(o, symbol)| json!({ "operation": o, "symbol": symbol }))
        .collect();
    let snapshots = store::list_snapshots(&state.pool, id).await?;
    let summary = store::summary(&state.pool, &pf).await?;
    Ok(Json(json!({
        "portfolio": pf,
        "summary": summary,
        "assets": assets,
        "operations": operations,
        "snapshots": snapshots
    })))
}

#[derive(Deserialize)]
struct UpdateBody {
    name: Option<String>,
    description: Option<String>,
    currency: Option<String>,
    auto_refresh: Option<bool>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::update_portfolio(
        &state.pool,
        id,
        b.name.as_deref().map(str::trim),
        b.description.as_deref().map(str::trim),
        b.currency.as_deref().map(str::trim),
        b.auto_refresh,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    Ok(Json(json!({ "portfolio": pf })))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    store::delete_portfolio(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn refresh(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    prices::refresh_portfolio(&state.pool, &pf).await?;
    let summary = store::summary(&state.pool, &pf).await?;
    Ok(Json(json!({ "summary": summary })))
}

/// Check every asset against its (possibly overridden) price source, persisting an ok/unresolved
/// status per asset. Returns the per-asset results so the client can drive the reconcile modal.
async fn reconcile(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let results = prices::reconcile_portfolio(&state.pool, &pf).await?;
    let unresolved = results.iter().filter(|r| r.status == "unresolved").count();
    Ok(Json(json!({ "results": results, "unresolved": unresolved })))
}

// ── Symbol search ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    /// "crypto" | "stock". "stock" also returns ETFs.
    kind: String,
}

async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let hits = match q.kind.as_str() {
        "crypto" => prices::search_crypto(&state.pool, &q.q).await?,
        "stock" | "etf" => prices::search_stock(&q.q).await?,
        _ => return Err(ApiError::bad_request("kind must be 'crypto' or 'stock'")),
    };
    Ok(Json(json!({ "results": hits })))
}

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddAssetBody {
    asset_class: String,
    provider: String,
    provider_id: String,
    symbol: String,
    #[serde(default)]
    name: String,
}

async fn add_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddAssetBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(b.provider.as_str(), "coingecko" | "yahoo") {
        return Err(ApiError::bad_request("unknown provider"));
    }
    if b.provider_id.trim().is_empty() {
        return Err(ApiError::bad_request("provider_id is required"));
    }
    store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let asset = store::add_asset(
        &state.pool,
        id,
        b.asset_class.trim(),
        b.provider.trim(),
        b.provider_id.trim(),
        b.symbol.trim(),
        b.name.trim(),
    )
    .await?;
    Ok(Json(json!({ "asset": asset })))
}

async fn asset_detail(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let asset = store::get_asset(&state.pool, asset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("asset not found"))?;
    let operations = store::list_operations(&state.pool, asset_id).await?;
    Ok(Json(json!({ "asset": asset, "operations": operations })))
}

async fn delete_asset(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_asset(&state.pool, asset_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Deserialize a JSON field so an *absent* key → None (leave unchanged) but an explicit `null` →
/// Some(None) (clear it). Needed to tell "don't touch spot_provider" from "remove the override".
fn double_option<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(d)?))
}

#[derive(Deserialize)]
struct PatchAssetBody {
    /// Absent = leave; null = clear override; "binance"|"kraken"|"coinbase"|"yahoo"|"coingecko" = set.
    #[serde(default, deserialize_with = "double_option")]
    spot_provider: Option<Option<String>>,
    /// Provider-specific ticker for the override (e.g. "BTCUSDT").
    spot_symbol: Option<String>,
    /// "ok" | "unresolved" | "manual". Typically "manual" (opt out) or "ok" (clear a manual flag).
    recon_status: Option<String>,
}

/// Patch an asset's price-source override and/or reconcile status. Validates the provider and
/// status enums here (the DB also constrains them). Returns the updated asset.
async fn patch_asset(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
    Json(b): Json<PatchAssetBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(Some(p)) = &b.spot_provider {
        if !matches!(p.as_str(), "coingecko" | "yahoo" | "binance" | "kraken" | "coinbase") {
            return Err(ApiError::bad_request("unknown spot_provider"));
        }
    }
    if let Some(s) = &b.recon_status {
        if !matches!(s.as_str(), "ok" | "unresolved" | "manual") {
            return Err(ApiError::bad_request("invalid recon_status"));
        }
    }
    let spot_provider = b.spot_provider.as_ref().map(|o| o.as_deref());
    let asset = store::update_asset(
        &state.pool,
        asset_id,
        spot_provider,
        b.spot_symbol.as_deref().map(str::trim),
        b.recon_status.as_deref(),
    )
    .await?
    .ok_or_else(|| ApiError::not_found("asset not found"))?;
    Ok(Json(json!({ "asset": asset })))
}

// ── Operations ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddOpBody {
    side: String,
    op_date: Option<String>,
    quantity: f64,
    price: f64,
    #[serde(default)]
    fee: f64,
    #[serde(default)]
    note: String,
}

async fn add_operation(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
    Json(b): Json<AddOpBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(b.side.as_str(), "buy" | "sell") {
        return Err(ApiError::bad_request("side must be 'buy' or 'sell'"));
    }
    if b.quantity <= 0.0 {
        return Err(ApiError::bad_request("quantity must be positive"));
    }
    store::get_asset(&state.pool, asset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("asset not found"))?;
    let date = parse_date(b.op_date.as_deref())?;
    let op = store::add_operation(
        &state.pool,
        asset_id,
        b.side.trim(),
        date,
        b.quantity,
        b.price,
        b.fee,
        b.note.trim(),
    )
    .await?;
    Ok(Json(json!({ "operation": op })))
}

async fn delete_operation(
    State(state): State<AppState>,
    Path(op_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_operation(&state.pool, op_id).await?;
    Ok(Json(json!({ "ok": true })))
}
