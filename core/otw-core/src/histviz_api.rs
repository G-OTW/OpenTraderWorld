//! Chart workspaces, per-instrument layouts and the chart's own instrument lists.
//!
//! The series and the live stream live in `histdata_api` (they are reads of market data);
//! what is here is the *arrangement*: which instruments the user has on screen, in what
//! grid, with which studies drawn on each.
//!
//! - `GET/PUT/DELETE /api/histviz/layout`      one instrument's layout, keyed by coordinates
//! - `GET/PUT       /api/histviz/layouts/{id}` the same thing addressed by dataset id (kept
//!                                             for clients that still hold one)
//! - `/api/histviz/workspaces*`                the grid: rows x columns and the panes in it
//! - `/api/histviz/lists*`                     chart-owned instrument lists for the rail
//! - `/api/histviz/alerts*`                    levels the server watches on closed bars
//!
//! **A layout belongs to an instrument, not to storage.** It used to be keyed on the dataset
//! id, so a symbol you charted without saving lost every indicator and drawing the moment you
//! left it. Coordinates are what the chart is addressed by everywhere else, so they are what
//! the layout is filed under too, and the dataset-addressed routes simply resolve the dataset
//! back to its coordinates.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::histdata as hist_store;
use otw_store::histviz as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/histviz/layout",
            get(get_layout).put(set_layout).delete(delete_layout),
        )
        // Same layout, addressed by the dataset id a stored instrument carries.
        .route(
            "/api/histviz/layouts/{id}",
            get(get_dataset_layout).put(set_dataset_layout),
        )
        .route(
            "/api/histviz/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/api/histviz/workspaces/{id}",
            get(get_workspace).put(update_workspace).delete(delete_workspace),
        )
        .route("/api/histviz/lists", get(list_lists).post(create_list))
        .route(
            "/api/histviz/lists/{id}",
            get(get_list).put(update_list).delete(delete_list),
        )
        .route("/api/histviz/lists/{id}/promote", post(promote_list))
        .route("/api/histviz/alerts", get(list_alerts).post(create_alert))
        .route(
            "/api/histviz/alerts/{id}",
            get(get_alert).put(update_alert).delete(delete_alert),
        )
}

// ── Instrument layouts ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CoordQuery {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
}

impl CoordQuery {
    fn key(&self) -> Result<String, ApiError> {
        coord_key(&self.provider, &self.asset_type, &self.ticker, &self.timeframe)
    }
}

/// The storage key for a set of coordinates, refusing the incomplete ones. A layout filed
/// under a blank ticker would be shared by every instrument that failed to name itself.
fn coord_key(
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> Result<String, ApiError> {
    for (name, value) in [
        ("provider", provider),
        ("asset_type", asset_type),
        ("ticker", ticker),
        ("timeframe", timeframe),
    ] {
        if value.trim().is_empty() {
            return Err(ApiError::bad_request(&format!("{name} is required")));
        }
    }
    Ok(store::coord_key(provider, asset_type, ticker, timeframe))
}

/// GET /api/histviz/layout?provider=&asset_type=&ticker=&timeframe= , the layout or `null`.
async fn get_layout(
    State(state): State<AppState>,
    Query(q): Query<CoordQuery>,
) -> Result<Json<Value>, ApiError> {
    let layout = store::get_layout(&state.pool, &q.key()?).await?;
    Ok(Json(layout.unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct LayoutBody {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
    layout: Value,
}

/// PUT /api/histviz/layout , replace it. The client owns the schema; we round-trip the JSON
/// object and only refuse something that is not one.
async fn set_layout(
    State(state): State<AppState>,
    Json(b): Json<LayoutBody>,
) -> Result<Json<Value>, ApiError> {
    if !b.layout.is_object() {
        return Err(ApiError::bad_request("layout must be a JSON object"));
    }
    let key = coord_key(&b.provider, &b.asset_type, &b.ticker, &b.timeframe)?;
    store::set_layout(&state.pool, &key, &b.layout).await?;
    Ok(Json(b.layout))
}

/// DELETE /api/histviz/layout , forget it (the pane goes back to the defaults).
async fn delete_layout(
    State(state): State<AppState>,
    Query(q): Query<CoordQuery>,
) -> Result<Json<Value>, ApiError> {
    let removed = store::delete_layout(&state.pool, &q.key()?).await?;
    Ok(Json(json!({ "deleted": removed })))
}

/// GET /api/histviz/layouts/{id} , the layout of the instrument that dataset holds.
async fn get_dataset_layout(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let key = dataset_key(&state, id).await?;
    let layout = store::get_layout(&state.pool, &key).await?;
    Ok(Json(layout.unwrap_or(Value::Null)))
}

/// PUT /api/histviz/layouts/{id} , same write through the dataset's coordinates.
async fn set_dataset_layout(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    if !body.is_object() {
        return Err(ApiError::bad_request("layout must be a JSON object"));
    }
    let key = dataset_key(&state, id).await?;
    store::set_layout(&state.pool, &key, &body).await?;
    Ok(Json(body))
}

async fn dataset_key(state: &AppState, id: Uuid) -> Result<String, ApiError> {
    let d = hist_store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset"))?;
    Ok(store::coord_key(&d.provider, &d.asset_type, &d.ticker, &d.timeframe))
}

// ── Workspaces ─────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct WorkspaceBody {
    name: Option<String>,
    /// Grid shape. 1x1 up to 3x4: a vertical split, a horizontal split and a 2x2 are the
    /// same model, so there is no list of named layouts to keep in sync.
    rows: Option<i32>,
    cols: Option<i32>,
    /// One entry per pane: cell, coordinates, per-pane layout override, link group.
    panes: Option<Value>,
    /// Splitter fractions and link-group colors.
    settings: Option<Value>,
    position: Option<i32>,
}

impl WorkspaceBody {
    /// Validate and fill in the defaults. The grid bounds are enforced here *and* by the
    /// table's check constraint: a 4x9 workspace is 36 live feeds, which is a mistake the
    /// server should refuse rather than discover on the provider's side.
    fn input(&self) -> Result<store::WorkspaceInput<'_>, ApiError> {
        let name = self.name.as_deref().unwrap_or("").trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("name is required"));
        }
        let rows = self.rows.unwrap_or(1);
        let cols = self.cols.unwrap_or(1);
        if !(1..=store::MAX_ROWS).contains(&rows) || !(1..=store::MAX_COLS).contains(&cols) {
            return Err(ApiError::bad_request(&format!(
                "a workspace grid is between 1x1 and {}x{}",
                store::MAX_ROWS,
                store::MAX_COLS
            )));
        }
        let panes = self.panes.as_ref().unwrap_or(&Value::Null);
        if !panes.is_null() && !panes.is_array() {
            return Err(ApiError::bad_request("panes must be an array"));
        }
        let settings = self.settings.as_ref().unwrap_or(&Value::Null);
        if !settings.is_null() && !settings.is_object() {
            return Err(ApiError::bad_request("settings must be a JSON object"));
        }
        Ok(store::WorkspaceInput {
            name,
            grid_rows: rows,
            grid_cols: cols,
            panes: if panes.is_null() { &EMPTY_ARRAY } else { panes },
            settings: if settings.is_null() { &EMPTY_OBJECT } else { settings },
            position: self.position.unwrap_or(0),
        })
    }
}

/// The two defaults a body may leave out, as long-lived values so `input` can borrow them.
static EMPTY_ARRAY: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!([]));
static EMPTY_OBJECT: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!({}));

async fn list_workspaces(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_workspaces(&state.pool).await?;
    Ok(Json(json!({ "workspaces": rows })))
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let w = store::get_workspace(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace"))?;
    Ok(Json(json!(w)))
}

async fn create_workspace(
    State(state): State<AppState>,
    Json(b): Json<WorkspaceBody>,
) -> Result<Json<Value>, ApiError> {
    let w = store::create_workspace(&state.pool, b.input()?).await?;
    Ok(Json(json!(w)))
}

async fn update_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<WorkspaceBody>,
) -> Result<Json<Value>, ApiError> {
    let w = store::update_workspace(&state.pool, id, b.input()?)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace"))?;
    Ok(Json(json!(w)))
}

async fn delete_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_workspace(&state.pool, id).await? {
        return Err(ApiError::not_found("workspace"));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ── Chart lists ────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ListBody {
    name: Option<String>,
    /// [{provider, asset_type, ticker, timeframe, connector_id, name}] in the user's order.
    items: Option<Value>,
    position: Option<i32>,
}

impl ListBody {
    fn parts(&self) -> Result<(&str, &Value, i32), ApiError> {
        let name = self.name.as_deref().unwrap_or("").trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("name is required"));
        }
        let items = self.items.as_ref().unwrap_or(&Value::Null);
        if !items.is_null() && !items.is_array() {
            return Err(ApiError::bad_request("items must be an array"));
        }
        Ok((
            name,
            if items.is_null() { &EMPTY_ARRAY } else { items },
            self.position.unwrap_or(0),
        ))
    }
}

async fn list_lists(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_lists(&state.pool).await?;
    Ok(Json(json!({ "lists": rows })))
}

async fn get_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let l = store::get_list(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("list"))?;
    Ok(Json(json!(l)))
}

async fn create_list(
    State(state): State<AppState>,
    Json(b): Json<ListBody>,
) -> Result<Json<Value>, ApiError> {
    let (name, items, position) = b.parts()?;
    let l = store::create_list(&state.pool, name, items, position).await?;
    Ok(Json(json!(l)))
}

async fn update_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ListBody>,
) -> Result<Json<Value>, ApiError> {
    let (name, items, position) = b.parts()?;
    let l = store::update_list(&state.pool, id, name, items, position)
        .await?
        .ok_or_else(|| ApiError::not_found("list"))?;
    Ok(Json(json!(l)))
}

async fn delete_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_list(&state.pool, id).await? {
        return Err(ApiError::not_found("list"));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// POST /api/histviz/lists/{id}/promote , copy a chart list into the Watchlists module.
///
/// The rail shows both sources, and only this route ever writes into the other module: a
/// list you built by clicking "add this instrument" stays a scratchpad until you say
/// otherwise. The chart list is left alone, so promoting twice is not destructive.
async fn promote_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<PromoteBody>,
) -> Result<Json<Value>, ApiError> {
    let l = store::get_list(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("list"))?;
    let items = l.items.as_array().cloned().unwrap_or_default();
    let name = b.name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&l.name);
    let wl = otw_store::watchlists::create_watchlist(&state.pool, name, "").await?;
    let mut added = 0usize;
    for it in &items {
        let ticker = it.get("ticker").and_then(Value::as_str).unwrap_or("").trim();
        if ticker.is_empty() {
            continue;
        }
        let provider = it.get("provider").and_then(Value::as_str).unwrap_or("").trim();
        let asset_type = it.get("asset_type").and_then(Value::as_str).unwrap_or("").trim();
        let label = it.get("name").and_then(Value::as_str).unwrap_or(ticker);
        otw_store::watchlists::add_item(
            &state.pool,
            wl.id,
            asset_class_of(asset_type),
            provider,
            ticker,
            ticker,
            label,
        )
        .await?;
        added += 1;
    }
    Ok(Json(json!({ "watchlist": wl, "added": added })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PromoteBody {
    /// Name for the new watchlist. Absent = the chart list's own name.
    name: Option<String>,
}

/// The Watchlists module files an item by asset *class*, the chart by asset *type*. Only
/// crypto and fx differ in name; everything else is an instrument on an exchange.
fn asset_class_of(asset_type: &str) -> &'static str {
    match asset_type {
        "crypto" => "crypto",
        "fx" | "forex" => "forex",
        "option" => "option",
        "future" => "future",
        "index" => "index",
        _ => "stock",
    }
}

// ── Alerts ─────────────────────────────────────────────────────────────────────
//
// A level on an instrument, watched by `histviz_alerts` with no browser open. This layer
// only validates the vocabulary; what a crossing *is* belongs to the evaluator, and the
// bookkeeping columns (when it last fired, what it last saw) are never writable from here.

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AlertBody {
    name: Option<String>,
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
    connector_id: Option<Uuid>,
    /// `price` (a price series) or `indicator` (a definition from the shared library).
    kind: Option<String>,
    /// Which price the level is compared against: close (default), high, low or open.
    source: Option<String>,
    indicator_id: Option<Uuid>,
    /// `above` and `below` are crossings, not states: an alert placed under the price does
    /// not fire until the price actually comes back through it.
    op: String,
    value: f64,
    /// Notification channels; empty = every channel the chart is granted.
    channels: Option<Vec<Uuid>>,
    repeat: Option<bool>,
    cooldown_secs: Option<i32>,
    enabled: Option<bool>,
}

const ALERT_KINDS: &[&str] = &["price", "indicator"];
const ALERT_OPS: &[&str] = &["above", "below", "crosses"];
const ALERT_SOURCES: &[&str] = &["close", "open", "high", "low"];

impl AlertBody {
    /// Validate the vocabulary and hand back the row to write, plus the channel list the
    /// input borrows (it has to outlive the call, so the caller owns it).
    fn parts(&self) -> Result<(store::AlertInput<'_>, Value), ApiError> {
        let kind = self.kind.as_deref().unwrap_or("price");
        if !ALERT_KINDS.contains(&kind) {
            return Err(ApiError::bad_request("kind must be price or indicator"));
        }
        if !ALERT_OPS.contains(&self.op.as_str()) {
            return Err(ApiError::bad_request("op must be above, below or crosses"));
        }
        let source = self.source.as_deref().unwrap_or("close");
        if !ALERT_SOURCES.contains(&source) {
            return Err(ApiError::bad_request("source must be close, open, high or low"));
        }
        if kind == "indicator" && self.indicator_id.is_none() {
            return Err(ApiError::bad_request(
                "an indicator alert needs the indicator it watches",
            ));
        }
        if !self.value.is_finite() {
            return Err(ApiError::bad_request("value must be a number"));
        }
        for (name, v) in [
            ("provider", &self.provider),
            ("asset_type", &self.asset_type),
            ("ticker", &self.ticker),
            ("timeframe", &self.timeframe),
        ] {
            if v.trim().is_empty() {
                return Err(ApiError::bad_request(&format!("{name} is required")));
            }
        }
        // The instrument must be one the chart can actually read, checked here rather than
        // discovered at 3 a.m. by a loop that can only write it into `last_error`.
        crate::histdata::validate_request(
            self.provider.trim(),
            self.asset_type.trim(),
            self.timeframe.trim(),
        )
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
        let channels = json!(self.channels.clone().unwrap_or_default());
        Ok((
            store::AlertInput {
                name: self.name.as_deref().unwrap_or("").trim(),
                provider: self.provider.trim(),
                asset_type: self.asset_type.trim(),
                ticker: self.ticker.trim(),
                timeframe: self.timeframe.trim(),
                connector_id: self.connector_id,
                kind,
                source,
                indicator_id: self.indicator_id,
                op: &self.op,
                value: self.value,
                channels: &EMPTY_ARRAY,
                repeat: self.repeat.unwrap_or(false),
                cooldown_secs: self.cooldown_secs.unwrap_or(0).max(0),
                enabled: self.enabled.unwrap_or(true),
            },
            channels,
        ))
    }
}

#[derive(Deserialize)]
struct AlertQuery {
    provider: Option<String>,
    asset_type: Option<String>,
    ticker: Option<String>,
    timeframe: Option<String>,
}

/// GET /api/histviz/alerts , every alert, or just one instrument's (what a pane draws).
async fn list_alerts(
    State(state): State<AppState>,
    Query(q): Query<AlertQuery>,
) -> Result<Json<Value>, ApiError> {
    let instrument = match (&q.provider, &q.asset_type, &q.ticker, &q.timeframe) {
        (Some(p), Some(a), Some(t), Some(tf)) => {
            Some((p.as_str(), a.as_str(), t.as_str(), tf.as_str()))
        }
        _ => None,
    };
    let rows = store::list_alerts(&state.pool, instrument).await?;
    Ok(Json(json!({ "alerts": rows })))
}

async fn get_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let a = store::get_alert(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("alert"))?;
    Ok(Json(json!(a)))
}

async fn create_alert(
    State(state): State<AppState>,
    Json(b): Json<AlertBody>,
) -> Result<Json<Value>, ApiError> {
    let (mut input, channels) = b.parts()?;
    input.channels = &channels;
    let a = store::create_alert(&state.pool, input).await?;
    Ok(Json(json!(a)))
}

async fn update_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AlertBody>,
) -> Result<Json<Value>, ApiError> {
    let (mut input, channels) = b.parts()?;
    input.channels = &channels;
    let a = store::update_alert(&state.pool, id, input)
        .await?
        .ok_or_else(|| ApiError::not_found("alert"))?;
    Ok(Json(json!(a)))
}

async fn delete_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_alert(&state.pool, id).await? {
        return Err(ApiError::not_found("alert"));
    }
    Ok(Json(json!({ "deleted": true })))
}
