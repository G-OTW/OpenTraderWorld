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
use crate::watchlists::{self, alerts, quotes};
use crate::{ApiError, AppState};
use otw_store::watchlist_alerts as alerts_store;
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
        .route("/api/watchlists/{id}/series", get(series))
        .route("/api/watchlists/{id}/correlation", get(correlation))
        .route("/api/watchlists/{id}/events", get(events))
        .route("/api/watchlists/{id}/news-counts", get(news_counts))
        .route("/api/watchlists/{id}/news-seen", post(news_seen))
        .route("/api/watchlists/{id}/reorder", post(reorder))
        .route("/api/watchlists/{id}/sections/rename", post(rename_section))
        .route("/api/watchlists/{id}/resolve-batch", post(resolve_batch))
        .route("/api/watchlists/{id}/items/batch", post(add_items_batch))
        .route(
            "/api/watchlists/items/{item_id}",
            axum::routing::patch(patch_item).delete(delete_item),
        )
        .route("/api/watchlists/items/{item_id}/alerts", post(add_alert))
        .route(
            "/api/watchlists/alerts/{alert_id}",
            axum::routing::patch(patch_alert).delete(delete_alert),
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
    let alerts = alerts_store::list_for_watchlist(&state.pool, id).await?;
    let charts = chart_links(&state, &wl, &items).await;
    Ok(Json(json!({ "watchlist": wl, "items": items, "alerts": alerts, "charts": charts })))
}

/// Chart coordinates per item (provider, asset type, ticker), for "Open in Chart" and the
/// stored-candle lookups. Items on the default crypto path have none and are absent.
async fn chart_links(
    state: &AppState,
    wl: &store::Watchlist,
    items: &[store::Item],
) -> std::collections::HashMap<Uuid, quotes::Coords> {
    let providers = quotes::resolved_providers(&state.pool, wl, items).await;
    items
        .iter()
        .filter_map(|i| {
            quotes::chart_coords(i, providers.get(&i.id).map(String::as_str)).map(|c| (i.id, c))
        })
        .collect()
}

/// Column ids a list can show (the header gear). Changes, trend and notes are the classic
/// table; the rest read quote fields that are null when the source does not serve them.
pub const COLUMNS: &[&str] = &[
    "c24", "c3", "c7", "c30", "spark", "range52", "notes", "mcap", "pe", "divy", "vol", "rsi",
    "dist52h", "targets",
];

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateBody {
    name: Option<String>,
    description: Option<String>,
    sync_enabled: Option<bool>,
    refresh_secs: Option<i32>,
    /// Quote source: absent = untouched, "" = back to the default providers, else a
    /// data-broker connector id.
    connector_id: Option<String>,
    /// Columns shown, ids from `COLUMNS`, in display order.
    columns: Option<Vec<String>>,
    /// Daily movers digest: "off" | "daily".
    digest: Option<String>,
    /// UTC hour (0 to 23) the digest goes out at or after.
    digest_hour: Option<i16>,
    /// Channels for the digest (granted to Watchlists); empty = every granted channel.
    digest_channels: Option<Vec<Uuid>>,
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
            let c = otw_store::connectors::get(&state.pool, cid)
                .await?
                .ok_or_else(|| ApiError::bad_request("unknown connector"))?;
            // Shared broker list: the connector still has to be granted to Watchlists.
            if !c.allows("watchlists") {
                return Err(ApiError::bad_request("connector is not enabled for watchlists"));
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
    if let Some(cols) = &b.columns {
        if let Some(bad) = cols.iter().find(|c| !COLUMNS.contains(&c.as_str())) {
            return Err(ApiError::bad_request(&format!("unknown column {bad:?}")));
        }
    }
    if let Some(d) = b.digest.as_deref() {
        if !matches!(d, "off" | "daily") {
            return Err(ApiError::bad_request("digest must be off or daily"));
        }
        // Demo mode sends nothing out.
        if d == "daily" && crate::demo::enabled() {
            return Err(ApiError::forbidden("the demo does not send digests"));
        }
    }
    if let Some(h) = b.digest_hour {
        if !(0..=23).contains(&h) {
            return Err(ApiError::bad_request("digest_hour must be between 0 and 23"));
        }
    }
    if let Some(ch) = &b.digest_channels {
        validate_channels(&state, ch).await?;
    }
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
    if let Some(cols) = &b.columns {
        let mut uniq: Vec<String> = Vec::new();
        for c in cols {
            if !uniq.contains(c) {
                uniq.push(c.clone());
            }
        }
        store::set_columns(&state.pool, id, &uniq).await?;
    }
    if b.digest.is_some() || b.digest_hour.is_some() || b.digest_channels.is_some() {
        store::set_digest(
            &state.pool,
            id,
            b.digest.as_deref().unwrap_or(&wl.digest),
            b.digest_hour.unwrap_or(wl.digest_hour),
            b.digest_channels.as_deref().unwrap_or(&wl.digest_channels),
        )
        .await?;
    }
    let wl = store::get_watchlist(&state.pool, id)
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
/// Alerts are evaluated on the fresh quotes too, so a manual refresh can fire one — the
/// same code path the loop runs, just triggered by hand.
async fn refresh(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    {
        let _guard = state.watchlists_refresh.lock().await;
        quotes::refresh_watchlist(&state.pool, &wl).await?;
    }
    if let Err(e) = alerts::evaluate(&state.pool, &alert_cx(&state), &wl).await {
        tracing::error!("alert evaluation after manual refresh failed: {e:#}");
    }
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let items = store::list_items(&state.pool, id).await?;
    let alerts = alerts_store::list_for_watchlist(&state.pool, id).await?;
    let charts = chart_links(&state, &wl, &items).await;
    Ok(Json(json!({ "watchlist": wl, "items": items, "alerts": alerts, "charts": charts })))
}

fn alert_cx(state: &AppState) -> watchlists::AlertCx {
    watchlists::AlertCx {
        cipher: state.cipher.clone(),
        http: state.http.clone(),
    }
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchQuery {
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
    /// Group inside the list; null or "" = ungrouped.
    #[serde(default, deserialize_with = "double_option")]
    section: Option<Option<String>>,
    /// red | orange | yellow | green | blue | purple; null = no flag.
    #[serde(default, deserialize_with = "double_option")]
    flag: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    target_buy: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    target_sell: Option<Option<f64>>,
    /// Virtual position: quantity, price and date (YYYY-MM-DD); null clears.
    #[serde(default, deserialize_with = "double_option")]
    virt_qty: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    virt_price: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    virt_date: Option<Option<String>>,
}

/// Absent field (None) vs explicit null (Some(None)): "leave it" vs "clear it".
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(d).map(Some)
}

/// A positive finite number or null; anything else is refused with the field's name.
fn positive(field: &str, v: Option<Option<f64>>) -> Result<Option<Option<f64>>, ApiError> {
    match v {
        Some(Some(x)) if !(x.is_finite() && x > 0.0) => {
            Err(ApiError::bad_request(&format!("{field} must be a positive number")))
        }
        other => Ok(other),
    }
}

/// Validate the row fields of a patch into the store's shape.
fn item_fields(b: &PatchItemBody) -> Result<store::ItemFields, ApiError> {
    let flag = match &b.flag {
        Some(Some(f)) if !store::FLAGS.contains(&f.as_str()) => {
            return Err(ApiError::bad_request(&format!(
                "flag must be one of {}",
                store::FLAGS.join(", ")
            )))
        }
        other => other.clone(),
    };
    let virt_date = match &b.virt_date {
        None => None,
        Some(None) => Some(None),
        Some(Some(d)) if d.trim().is_empty() => Some(None),
        Some(Some(d)) => {
            let fmt = time::macros::format_description!("[year]-[month]-[day]");
            let date = time::Date::parse(d.trim(), &fmt)
                .map_err(|_| ApiError::bad_request("virt_date must be YYYY-MM-DD"))?;
            if date > time::OffsetDateTime::now_utc().date() {
                return Err(ApiError::bad_request("virt_date cannot be in the future"));
            }
            Some(Some(date))
        }
    };
    Ok(store::ItemFields {
        section: b.section.as_ref().map(|s| store::normalize_section(s.as_deref())),
        flag,
        target_buy: positive("target_buy", b.target_buy)?,
        target_sell: positive("target_sell", b.target_sell)?,
        virt_qty: positive("virt_qty", b.virt_qty)?,
        virt_price: positive("virt_price", b.virt_price)?,
        virt_date,
    })
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
            let c = otw_store::connectors::get(&state.pool, cid)
                .await?
                .ok_or_else(|| ApiError::bad_request("unknown connector"))?;
            if !c.allows("watchlists") {
                return Err(ApiError::bad_request("connector is not enabled for watchlists"));
            }
        }
    }
    let fields = item_fields(&b)?;
    if !fields.is_empty() {
        store::update_item_fields(&state.pool, item_id, &fields)
            .await?
            .ok_or_else(|| ApiError::not_found("item not found"))?;
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

// ── Price alerts ──────────────────────────────────────────────────────────────

/// Longest window we accept, in seconds (365 days). Beyond that a "since now" question
/// stops being one, and a rolling look-back would want stored history we never keep.
const MAX_WINDOW_SECS: i32 = 365 * 86_400;

/// Validate an alert payload against what the metric actually supports, so a nonsensical
/// combination is refused at the edge rather than silently never firing.
fn validate_alert(input: &alerts_store::AlertInput) -> Result<(), ApiError> {
    if !alerts_store::METRICS.contains(&input.metric.as_str()) {
        return Err(ApiError::bad_request("metric must be price, pct or usd"));
    }
    if !alerts_store::BASES.contains(&input.basis.as_str()) {
        return Err(ApiError::bad_request("basis must be anchor or rolling"));
    }
    if !alerts_store::directions_for(&input.metric).contains(&input.direction.as_str()) {
        return Err(ApiError::bad_request(&format!(
            "direction for {} must be one of {}",
            input.metric,
            alerts_store::directions_for(&input.metric).join(", ")
        )));
    }
    if !input.threshold.is_finite() {
        return Err(ApiError::bad_request("threshold must be a number"));
    }
    // A price alert takes a level (always positive); a variation takes a magnitude, and a
    // zero one would fire on the first tick forever.
    if input.metric == "price" {
        if input.threshold <= 0.0 {
            return Err(ApiError::bad_request("price threshold must be positive"));
        }
    } else if input.threshold <= 0.0 {
        return Err(ApiError::bad_request("the move must be greater than zero"));
    }
    if !(0..=MAX_WINDOW_SECS).contains(&input.window_secs) {
        return Err(ApiError::bad_request("window is out of range"));
    }
    // A rolling window with no look-back has nothing to compare against.
    if input.metric != "price" && input.basis == "rolling" && input.window_secs <= 0 {
        return Err(ApiError::bad_request("a rolling alert needs a window"));
    }
    if input.cooldown_secs < 0 {
        return Err(ApiError::bad_request("cooldown cannot be negative"));
    }
    Ok(())
}

/// Reject channel ids that don't exist or that the user hasn't granted to Watchlists, so
/// neither a typo nor a stale pick can silently mute an alert. An empty selection is the
/// documented "every enabled channel" default.
async fn validate_channels(state: &AppState, ids: &[Uuid]) -> Result<(), ApiError> {
    for id in ids {
        match otw_store::notif_channels::get(&state.pool, *id).await? {
            Some(ch) if ch.allows("watchlists") => {}
            Some(_) => {
                return Err(ApiError::bad_request(
                    "this notification channel is not allowed for Watchlists",
                ))
            }
            None => return Err(ApiError::bad_request("unknown notification channel")),
        }
    }
    Ok(())
}

/// The item's current price — the reference an anchor alert freezes at arm time.
fn item_ref_price(item: &store::Item) -> Option<f64> {
    item.quote
        .get("price_usd")
        .and_then(Value::as_f64)
        .filter(|p| p.is_finite())
}

async fn add_alert(
    State(state): State<AppState>,
    Path(item_id): Path<Uuid>,
    Json(b): Json<alerts_store::AlertInput>,
) -> Result<Json<Value>, ApiError> {
    validate_alert(&b)?;
    validate_channels(&state, &b.channel_ids).await?;
    let item = store::get_item(&state.pool, item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("item not found"))?;
    let alert = alerts_store::add(&state.pool, item_id, &b, item_ref_price(&item)).await?;
    Ok(Json(json!({ "alert": alert })))
}

async fn patch_alert(
    State(state): State<AppState>,
    Path(alert_id): Path<Uuid>,
    Json(b): Json<alerts_store::AlertInput>,
) -> Result<Json<Value>, ApiError> {
    validate_alert(&b)?;
    validate_channels(&state, &b.channel_ids).await?;
    let existing = alerts_store::get(&state.pool, alert_id)
        .await?
        .ok_or_else(|| ApiError::not_found("alert not found"))?;
    let item = store::get_item(&state.pool, existing.item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("item not found"))?;
    let alert = alerts_store::update(&state.pool, alert_id, &b, item_ref_price(&item))
        .await?
        .ok_or_else(|| ApiError::not_found("alert not found"))?;
    Ok(Json(json!({ "alert": alert })))
}

async fn delete_alert(
    State(state): State<AppState>,
    Path(alert_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !alerts_store::delete(&state.pool, alert_id).await? {
        return Err(ApiError::not_found("alert not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Import from a Portfolio Tracker portfolio ─────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ImportBody {
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

// ── Compare chart and correlation ─────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct WindowQuery {
    /// 1m | 3m | 1y.
    #[serde(default = "default_window")]
    window: String,
}

fn default_window() -> String {
    "1m".into()
}

async fn load_list(state: &AppState, id: Uuid) -> Result<(store::Watchlist, Vec<store::Item>), ApiError> {
    let wl = store::get_watchlist(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    let items = store::list_items(&state.pool, id).await?;
    Ok((wl, items))
}

/// Daily closes of every symbol over the window: 1M from the cached quote history, 3M and
/// 1Y from stored candles. Symbols the window is not covered for are listed with why.
async fn series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<Value>, ApiError> {
    let days = watchlists::series::window_days(&q.window)
        .ok_or_else(|| ApiError::bad_request("window must be 1m, 3m or 1y"))?;
    let (wl, items) = load_list(&state, id).await?;
    let (series, not_covered) = watchlists::series::list_series(&state.pool, &wl, &items, days).await;
    Ok(Json(json!({ "window": q.window, "series": series, "not_covered": not_covered })))
}

/// Pairwise Pearson correlation of daily returns over the window. A pair with fewer than
/// `MIN_OVERLAP` common returns is null, not a number drawn from a handful of days.
async fn correlation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<Value>, ApiError> {
    let days = watchlists::series::window_days(&q.window)
        .ok_or_else(|| ApiError::bad_request("window must be 1m, 3m or 1y"))?;
    let (wl, items) = load_list(&state, id).await?;
    let (series, not_covered) = watchlists::series::list_series(&state.pool, &wl, &items, days).await;
    let matrix: Vec<Vec<Value>> = series
        .iter()
        .map(|a| {
            series
                .iter()
                .map(|b| match watchlists::series::pearson_common(&a.points, &b.points) {
                    Some((r, n)) => json!({ "r": r, "n": n }),
                    None => Value::Null,
                })
                .collect()
        })
        .collect();
    let symbols: Vec<Value> =
        series.iter().map(|s| json!({ "item_id": s.item_id, "symbol": s.symbol })).collect();
    Ok(Json(json!({
        "window": q.window,
        "symbols": symbols,
        "matrix": matrix,
        "min_overlap": watchlists::series::MIN_OVERLAP,
        "not_covered": not_covered,
    })))
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Upcoming earnings, ex-dividend and split dates from stored Fundamentals snapshots, with a
/// per-symbol status that says when a symbol has no event data at all.
async fn events(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let (_, items) = load_list(&state, id).await?;
    let rows: Vec<(Uuid, String, String)> = items
        .iter()
        .map(|i| {
            let subject = if i.provider == "yahoo" {
                crate::fundamentals::edgar::normalize_ticker(&i.provider_id)
            } else {
                String::new()
            };
            (i.id, i.symbol.clone(), subject)
        })
        .collect();
    let subjects: Vec<String> = rows.iter().map(|r| r.2.clone()).filter(|s| !s.is_empty()).collect();
    let snaps = store::event_snapshots(&state.pool, &subjects).await?;
    let today = time::OffsetDateTime::now_utc().date().to_string();
    let (events, status) = watchlists::events::build(&rows, &snaps, &today);
    Ok(Json(json!({ "events": events, "status": status })))
}

// ── News badge ────────────────────────────────────────────────────────────────

/// News & Research items since the list's reset point whose title names each symbol (the
/// ticker as a word, or the company name without its legal form).
async fn news_counts(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let (wl, items) = load_list(&state, id).await?;
    let rows: Vec<(Uuid, String, String)> = items
        .iter()
        .map(|i| {
            let (s, n) = store::news_patterns(&i.symbol, &i.name);
            (i.id, s, n)
        })
        .collect();
    let counts: std::collections::HashMap<Uuid, i64> =
        store::news_counts(&state.pool, &rows, wl.news_seen_at).await?.into_iter().collect();
    let seen_at = serde_json::to_value(&wl).ok().and_then(|v| v.get("news_seen_at").cloned());
    Ok(Json(json!({ "counts": counts, "seen_at": seen_at })))
}

async fn news_seen(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    store::mark_news_seen(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("watchlist not found"))?;
    Ok(Json(json!({ "ok": true })))
}

// ── Sections and order ────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ReorderRow {
    id: Uuid,
    #[serde(default)]
    section: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ReorderBody {
    /// Every item of the list, in the new display order, each with its section.
    items: Vec<ReorderRow>,
}

async fn reorder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ReorderBody>,
) -> Result<Json<Value>, ApiError> {
    let (_, items) = load_list(&state, id).await?;
    let rows: Vec<(Uuid, Option<String>)> = b
        .items
        .iter()
        .map(|r| (r.id, store::normalize_section(r.section.as_deref())))
        .collect();
    let existing: Vec<Uuid> = items.iter().map(|i| i.id).collect();
    store::check_reorder(&existing, &rows).map_err(|e| ApiError::bad_request(&e))?;
    store::reorder(&state.pool, id, &rows).await?;
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RenameSectionBody {
    from: String,
    /// New name; "" ungroups the section's rows.
    #[serde(default)]
    to: String,
}

async fn rename_section(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<RenameSectionBody>,
) -> Result<Json<Value>, ApiError> {
    load_list(&state, id).await?;
    let to = store::normalize_section(Some(&b.to));
    let moved = store::rename_section(&state.pool, id, b.from.trim(), to.as_deref()).await?;
    if moved == 0 {
        return Err(ApiError::not_found("no row is in that section"));
    }
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "items": items })))
}

// ── Paste tickers ─────────────────────────────────────────────────────────────

/// Most tickers one paste may carry: each one is a provider search.
const MAX_PASTE: usize = 50;

/// Split a pasted list on commas, semicolons, whitespace and newlines, drop empties and
/// case-insensitive duplicates, keep the first spelling and the order.
pub fn parse_tickers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let t = tok.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '$');
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

/// Pick the one instrument a pasted token names, or say why there is none. Only an exact
/// match counts (symbol, or CoinGecko id). Several exact matches are ambiguous: the token
/// stays unresolved and carries them as candidates (search order, so the pinned coin of a
/// well-known ticker comes first), for the user to choose; nothing is picked for them.
pub fn pick_exact(
    token: &str,
    hits: &[prices::SearchHit],
) -> Result<prices::SearchHit, (String, Vec<prices::SearchHit>)> {
    if let Some(h) = hits.iter().find(|h| h.provider == "coingecko" && h.provider_id.eq_ignore_ascii_case(token)) {
        return Ok(h.clone());
    }
    let exact: Vec<&prices::SearchHit> = hits.iter().filter(|h| h.symbol.eq_ignore_ascii_case(token)).collect();
    match exact.len() {
        1 => Ok(exact[0].clone()),
        0 => Err((format!("no instrument has the exact symbol {token}: search it in the Search tab"), vec![])),
        n => Err((
            format!("{token} matches {n} instruments: choose one"),
            exact.iter().take(5).map(|h| (*h).clone()).collect(),
        )),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ResolveBatchBody {
    text: String,
    /// "crypto" | "stock" (stock also resolves ETFs).
    kind: String,
}

/// Resolve every pasted ticker; nothing is added. The answer splits resolved instruments
/// from unresolved tokens, each with the fix.
async fn resolve_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ResolveBatchBody>,
) -> Result<Json<Value>, ApiError> {
    load_list(&state, id).await?;
    if !matches!(b.kind.as_str(), "crypto" | "stock") {
        return Err(ApiError::bad_request("kind must be 'crypto' or 'stock'"));
    }
    let tokens = parse_tickers(&b.text);
    if tokens.is_empty() {
        return Err(ApiError::bad_request("paste at least one ticker"));
    }
    if tokens.len() > MAX_PASTE {
        return Err(ApiError::bad_request(&format!(
            "{} tickers pasted, at most {MAX_PASTE} at a time",
            tokens.len()
        )));
    }
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for tok in &tokens {
        let hits = match b.kind.as_str() {
            "crypto" => prices::search_crypto(&state.pool, tok).await,
            _ => prices::search_stock(tok).await,
        };
        let hits = match hits {
            Ok(h) => h,
            Err(e) => {
                unresolved.push(json!({ "token": tok, "reason": format!("search failed: {e:#}; try again") }));
                continue;
            }
        };
        match pick_exact(tok, &hits) {
            Ok(h) => resolved.push(json!({ "token": tok, "hit": h })),
            Err((reason, candidates)) => {
                unresolved.push(json!({ "token": tok, "reason": reason, "candidates": candidates }))
            }
        }
    }
    Ok(Json(json!({ "resolved": resolved, "unresolved": unresolved })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddBatchBody {
    items: Vec<AddItemBody>,
}

/// Add the confirmed instruments of a paste, then quote the list once.
async fn add_items_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddBatchBody>,
) -> Result<Json<Value>, ApiError> {
    let (wl, _) = load_list(&state, id).await?;
    if b.items.is_empty() || b.items.len() > MAX_PASTE {
        return Err(ApiError::bad_request(&format!("send between 1 and {MAX_PASTE} instruments")));
    }
    for i in &b.items {
        if !matches!(i.provider.as_str(), "coingecko" | "yahoo")
            || !matches!(i.asset_class.as_str(), "crypto" | "stock" | "etf")
            || i.provider_id.trim().is_empty()
        {
            return Err(ApiError::bad_request(&format!("{} is not a resolved instrument", i.symbol)));
        }
    }
    for i in &b.items {
        store::add_item(
            &state.pool,
            id,
            i.asset_class.trim(),
            i.provider.trim(),
            i.provider_id.trim(),
            i.symbol.trim(),
            i.name.trim(),
        )
        .await?;
    }
    {
        let _guard = state.watchlists_refresh.lock().await;
        if let Err(e) = quotes::refresh_watchlist(&state.pool, &wl).await {
            tracing::warn!("refresh after paste failed: {e:#}");
        }
    }
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "added": b.items.len(), "items": items })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(provider: &str, id: &str, symbol: &str, name: &str) -> prices::SearchHit {
        prices::SearchHit {
            asset_class: if provider == "yahoo" { "stock".into() } else { "crypto".into() },
            provider: provider.into(),
            provider_id: id.into(),
            symbol: symbol.into(),
            name: name.into(),
        }
    }

    #[test]
    fn paste_splits_on_commas_newlines_and_spaces_and_drops_duplicates() {
        assert_eq!(
            parse_tickers("AAPL, msft\nNVDA  TSLA;aapl\r\n\t$AMD,,\"META\""),
            vec!["AAPL", "msft", "NVDA", "TSLA", "AMD", "META"]
        );
        assert!(parse_tickers(" ,\n ").is_empty());
    }

    #[test]
    fn only_an_exact_single_match_resolves() {
        let hits = vec![hit("yahoo", "AAPL", "AAPL", "Apple Inc."), hit("yahoo", "AAPL.MX", "AAPL.MX", "Apple Inc.")];
        assert_eq!(pick_exact("aapl", &hits).unwrap().provider_id, "AAPL");
        // A prefix or a name is not a match, and offers nothing to choose from.
        let (_, cands) = pick_exact("APP", &hits).unwrap_err();
        assert!(cands.is_empty());
        // Two coins sharing a ticker are ambiguous: both offered, none picked.
        let coins = vec![hit("coingecko", "foo-a", "FOO", "Foo A"), hit("coingecko", "foo-b", "FOO", "Foo B")];
        let (reason, cands) = pick_exact("FOO", &coins).unwrap_err();
        assert!(reason.contains("matches 2"));
        assert_eq!(cands.len(), 2);
        // A CoinGecko id is exact too.
        assert_eq!(pick_exact("foo-a", &coins).unwrap().provider_id, "foo-a");
    }

    #[test]
    fn unknown_columns_are_not_in_the_catalog() {
        assert!(COLUMNS.contains(&"range52"));
        assert!(!COLUMNS.contains(&"price"));
    }

    #[test]
    fn row_fields_are_validated() {
        let body = |v: Value| serde_json::from_value::<PatchItemBody>(v).unwrap();
        assert!(item_fields(&body(json!({ "flag": "pink" }))).is_err());
        assert!(item_fields(&body(json!({ "target_buy": -1.0 }))).is_err());
        assert!(item_fields(&body(json!({ "virt_date": "2026-13-01" }))).is_err());
        assert!(item_fields(&body(json!({ "virt_date": "2999-01-01" }))).is_err());
        let Ok(f) = item_fields(&body(json!({ "flag": null, "section": "  core ", "target_sell": 120.5 }))) else {
            panic!("valid fields refused")
        };
        assert_eq!(f.flag, Some(None));
        assert_eq!(f.section, Some(Some("core".into())));
        assert_eq!(f.target_sell, Some(Some(120.5)));
        assert!(f.target_buy.is_none());
        assert!(item_fields(&body(json!({}))).is_ok_and(|f| f.is_empty()));
    }
}
