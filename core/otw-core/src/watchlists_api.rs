//! HTTP API for the Watchlists module.
//!
//! Watchlists → items (pinned provider symbols with a cached quote). The listing endpoint
//! returns lists with item counts; the detail endpoint returns a list's items with their
//! cached quotes (price, 24h/3d/7d/30d changes, sparkline). Symbol search reuses the
//! Portfolio Tracker's resolution (CoinGecko for crypto, Yahoo for stocks/ETFs). Lists can
//! be seeded from a curated template or imported from a Portfolio Tracker portfolio
//! (idempotent upsert — re-importing reconciles instead of duplicating). Refresh re-quotes
//! every item; a background loop does the same for sync-enabled lists on their own cadence.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::portfolios::prices;
use crate::watchlists::{self, quotes};
use crate::{ApiError, AppState};
use otw_store::watchlists as store;

/// Bounds for the per-list auto-refresh interval. On the default free providers the floor
/// is 60 s (don't hammer them); a list pinned to the user's own Historical Data connector
/// may go down to 5 s — the user opted in knowing their plan's limits. Ceiling is one day.
const MIN_REFRESH_SECS: i32 = 60;
const MIN_REFRESH_SECS_CUSTOM: i32 = 5;
const MAX_REFRESH_SECS: i32 = 86_400;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/watchlists", get(list).post(create))
        .route("/api/watchlists/search", get(search))
        .route("/api/watchlists/templates", get(templates))
        .route("/api/watchlists/{id}", get(detail).patch(update).delete(remove))
        .route("/api/watchlists/{id}/refresh", post(refresh))
        .route("/api/watchlists/{id}/items", post(add_item))
        .route("/api/watchlists/{id}/import", post(import_portfolio))
        .route(
            "/api/watchlists/items/{item_id}",
            axum::routing::patch(patch_item).delete(delete_item),
        )
}

// ── Watchlists ────────────────────────────────────────────────────────────────

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let lists = store::list_watchlists(&state.pool).await?;
    let counts: std::collections::HashMap<Uuid, i64> =
        store::item_counts(&state.pool).await?.into_iter().collect();
    let out: Vec<Value> = lists
        .iter()
        .map(|w| {
            let mut v = serde_json::to_value(w).unwrap_or_default();
            v["item_count"] = json!(counts.get(&w.id).copied().unwrap_or(0));
            v
        })
        .collect();
    Ok(Json(json!({ "watchlists": out })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    /// Optional starter template id (see GET /api/watchlists/templates).
    template: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let template = match b.template.as_deref().filter(|t| !t.is_empty()) {
        Some(id) => Some(
            watchlists::template_by_id(id).ok_or_else(|| ApiError::bad_request("unknown template"))?,
        ),
        None => None,
    };
    let name = match (b.name.trim(), template) {
        ("", Some(t)) => t.name,
        ("", None) => return Err(ApiError::bad_request("name is required")),
        (n, _) => n,
    };
    let wl = store::create_watchlist(&state.pool, name, b.description.trim()).await?;
    if let Some(t) = template {
        for s in t.symbols {
            store::add_item(
                &state.pool,
                wl.id,
                s.asset_class,
                s.provider,
                s.provider_id,
                s.symbol,
                s.name,
            )
            .await?;
        }
        // Quote the seeded symbols right away so the new list opens populated, not blank.
        // Failures are non-fatal — the next refresh will fill the gaps.
        let _guard = state.watchlists_refresh.lock().await;
        if let Err(e) = quotes::refresh_watchlist(&state.pool, &wl).await {
            tracing::warn!("initial refresh of templated watchlist failed: {e:#}");
        }
    }
    Ok(Json(json!({ "watchlist": wl })))
}

async fn detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "watchlist": wl, "items": items })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateBody {
    name: Option<String>,
    description: Option<String>,
    sync_enabled: Option<bool>,
    refresh_secs: Option<i32>,
    /// Quote source: absent = untouched, "" = back to the default providers, else a
    /// Historical Data connector id.
    connector_id: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(name) = &b.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("name cannot be empty"));
        }
    }
    let current = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let connector: Option<Option<Uuid>> = match b.connector_id.as_deref() {
        None => None,
        Some("") => Some(None),
        Some(s) => {
            let cid = Uuid::parse_str(s)
                .map_err(|_| ApiError::bad_request("invalid connector_id"))?;
            let c = otw_store::histdata::get_connector(&state.pool, cid)
                .await?
                .ok_or_else(|| ApiError::bad_request("unknown connector"))?;
            // Watchlists only quote through its own connector namespace.
            if c.scope != "watchlists" {
                return Err(ApiError::bad_request("unknown connector"));
            }
            Some(Some(cid))
        }
    };
    // The refresh floor follows the quote source the list will have after this patch.
    let effective_connector = connector.unwrap_or(current.connector_id);
    let floor = if effective_connector.is_some() { MIN_REFRESH_SECS_CUSTOM } else { MIN_REFRESH_SECS };
    if let Some(secs) = b.refresh_secs {
        if !(floor..=MAX_REFRESH_SECS).contains(&secs) {
            return Err(ApiError::bad_request(
                &format!("refresh_secs must be between {floor} and {MAX_REFRESH_SECS}"),
            ));
        }
    }
    // Dropping back to the default providers silently lifts a sub-minute cadence to the
    // free-provider floor instead of erroring.
    let refresh_secs = match (b.refresh_secs, effective_connector) {
        (Some(s), _) => Some(s),
        (None, None) if current.refresh_secs < MIN_REFRESH_SECS => Some(MIN_REFRESH_SECS),
        _ => None,
    };
    let wl = store::update_watchlist(
        &state.pool,
        id,
        b.name.as_deref().map(str::trim),
        b.description.as_deref().map(str::trim),
        b.sync_enabled,
        refresh_secs,
        connector,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    Ok(Json(json!({ "watchlist": wl })))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_watchlist(&state.pool, id).await? {
        return Err(ApiError::not_found("watchlist not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Re-quote every item now (manual refresh). Serialized against the background loop.
async fn refresh(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    {
        let _guard = state.watchlists_refresh.lock().await;
        quotes::refresh_watchlist(&state.pool, &wl).await?;
    }
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "watchlist": wl, "items": items })))
}

// ── Templates & symbol search ─────────────────────────────────────────────────

async fn templates() -> Json<Value> {
    let out: Vec<Value> = watchlists::TEMPLATES
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "count": t.symbols.len(),
                "symbols": t.symbols.iter().map(|s| s.symbol).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(json!({ "templates": out }))
}

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

// ── Items ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddItemBody {
    asset_class: String,
    provider: String,
    provider_id: String,
    symbol: String,
    #[serde(default)]
    name: String,
}

async fn add_item(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddItemBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(b.provider.as_str(), "coingecko" | "yahoo") {
        return Err(ApiError::bad_request("unknown provider"));
    }
    if !matches!(b.asset_class.as_str(), "crypto" | "stock" | "etf") {
        return Err(ApiError::bad_request("unknown asset_class"));
    }
    if b.provider_id.trim().is_empty() {
        return Err(ApiError::bad_request("provider_id is required"));
    }
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let item = store::add_item(
        &state.pool,
        id,
        b.asset_class.trim(),
        b.provider.trim(),
        b.provider_id.trim(),
        b.symbol.trim(),
        b.name.trim(),
    )
    .await?;
    // Quote it immediately so the row lands with a price; tolerate a source hiccup.
    if let Err(e) = quotes::refresh_item(&state.pool, &wl, &item).await {
        tracing::warn!("initial quote for {} failed: {e:#}", item.symbol);
    }
    let item = store::get_item(&state.pool, item.id)
        .await?
        .ok_or_else(|| ApiError::not_found("item not found"))?;
    Ok(Json(json!({ "item": item })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PatchItemBody {
    notes: Option<String>,
    position: Option<f64>,
    /// Ticker in the resolved connector's symbol format; "" reverts to the derived default.
    quote_ticker: Option<String>,
    /// Per-item source: "" = follow the list default, "auto" = force the default
    /// providers, else a Historical Data connector id.
    quote_source: Option<String>,
}

async fn patch_item(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
    Json(b): Json<PatchItemBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(src) = b.quote_source.as_deref().map(str::trim) {
        if !matches!(src, "" | "auto") {
            let cid = Uuid::parse_str(src)
                .map_err(|_| ApiError::bad_request("invalid quote_source"))?;
            let c = otw_store::histdata::get_connector(&state.pool, cid)
                .await?
                .ok_or_else(|| ApiError::bad_request("unknown connector"))?;
            if c.scope != "watchlists" {
                return Err(ApiError::bad_request("unknown connector"));
            }
        }
    }
    let item = store::update_item(
        &state.pool,
        item_id,
        b.notes.as_deref(),
        b.position,
        b.quote_ticker.as_deref().map(str::trim),
        b.quote_source.as_deref().map(str::trim),
    )
    .await?
    .ok_or_else(|| ApiError::not_found("item not found"))?;
    // A changed source/ticker re-quotes right away so the user sees whether it resolves;
    // a failure lands in quote.error rather than failing the patch.
    if b.quote_ticker.is_some() || b.quote_source.is_some() {
        if let Some(wl) = store::get_watchlist(&state.pool, item.watchlist_id).await? {
            let _guard = state.watchlists_refresh.lock().await;
            if let Err(e) = quotes::refresh_item(&state.pool, &wl, &item).await {
                tracing::warn!("re-quote after source change for {} failed: {e:#}", item.symbol);
            }
        }
        let item = store::get_item(&state.pool, item.id)
            .await?
            .ok_or_else(|| ApiError::not_found("item not found"))?;
        return Ok(Json(json!({ "item": item })));
    }
    Ok(Json(json!({ "item": item })))
}

async fn delete_item(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_item(&state.pool, item_id).await? {
        return Err(ApiError::not_found("item not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Import from a Portfolio Tracker portfolio ─────────────────────────────────

#[derive(Deserialize)]
struct ImportBody {
    portfolio_id: Uuid,
}

/// Copy a portfolio's assets into the watchlist (same provider/provider_id scheme). Upsert
/// semantics reconcile: symbols already on the list are refreshed, not duplicated. Newly
/// landed items are quoted inline so the table fills in one round-trip.
async fn import_portfolio(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    otw_store::portfolios::get_portfolio(&state.pool, b.portfolio_id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let assets = otw_store::portfolios::list_assets(&state.pool, b.portfolio_id).await?;
    if assets.is_empty() {
        return Err(ApiError::bad_request("portfolio has no assets to import"));
    }
    let mut added = 0usize;
    for a in &assets {
        store::add_item(
            &state.pool,
            id,
            &a.asset_class,
            &a.provider,
            &a.provider_id,
            &a.symbol,
            &a.name,
        )
        .await?;
        added += 1;
    }
    {
        let _guard = state.watchlists_refresh.lock().await;
        if let Err(e) = quotes::refresh_watchlist(&state.pool, &wl).await {
            tracing::warn!("post-import refresh failed: {e:#}");
        }
    }
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "imported": added, "items": items })))
}
