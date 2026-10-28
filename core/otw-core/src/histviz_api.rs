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
//! - `/api/histviz/alerts*`                    series pairs the server watches, polled or live
//! - `POST /api/histviz/scripts/run`         a chart's scripts run here, answers cached
//! - `POST /api/histviz/alerts/presence`       the app is open (keeps `app` alerts live)
//! - `POST /api/histviz/alerts/pause-all`      disable every armed alert
//!
//! **A layout belongs to an instrument, not to storage.** It used to be keyed on the dataset
//! id, so a symbol you charted without saving lost every indicator and drawing the moment you
//! left it. Coordinates are what the chart is addressed by everywhere else, so they are what
//! the layout is filed under too, and the dataset-addressed routes simply resolve the dataset
//! back to its coordinates.

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::backtest::Operand;
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
        .route("/api/histviz/alerts/presence", post(alert_presence))
        .route("/api/histviz/alerts/pause-all", post(pause_all_alerts))
        .route(
            "/api/histviz/alerts/{id}",
            get(get_alert).put(update_alert).delete(delete_alert),
        )
        .route(
            "/api/histviz/scripts/run",
            post(run_scripts).layer(axum::extract::DefaultBodyLimit::max(RUN_BODY_BYTES)),
        )
}

// ── Chart script runs ───────────────────────────────────────────────────────────

/// Bars a chart may send, scripts per request, and the body that carries them.
const RUN_MAX_BARS: usize = 50_000;
const RUN_MAX_SCRIPTS: usize = 12;
const RUN_BODY_BYTES: usize = 24 << 20;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ScriptRunBody {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// The bars the chart shows, oldest first: open times in epoch ms, then prices. An empty
    /// `volume` means the instrument has none.
    time: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    #[serde(default)]
    volume: Vec<f64>,
    scripts: Vec<ScriptRunItem>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ScriptRunItem {
    /// Echoed back with the answer.
    key: String,
    source: String,
    /// The run context the WASM host takes: `{syminfo, timeframe, inputs}` as JSON.
    ctx: String,
}

/// POST /api/histviz/scripts/run — run a chart's scripts on the server over the stored warmup
/// and the bars the chart shows. Each answer is what the WASM host's run answers, the output
/// cut to the chart's bars; answers are cached, so a chart reopened on the same bars, script
/// and inputs costs nothing. `{results: [{key, answer}]}`.
async fn run_scripts(
    State(state): State<AppState>,
    Json(b): Json<ScriptRunBody>,
) -> Result<axum::response::Response, ApiError> {
    let n = b.time.len();
    if n == 0 || n > RUN_MAX_BARS {
        return Err(ApiError::bad_request(&format!("time holds 1 to {RUN_MAX_BARS} bars")));
    }
    if [&b.open, &b.high, &b.low, &b.close].iter().any(|c| c.len() != n) || !(b.volume.is_empty() || b.volume.len() == n) {
        return Err(ApiError::bad_request("every price column has one value per bar"));
    }
    if b.scripts.is_empty() || b.scripts.len() > RUN_MAX_SCRIPTS {
        return Err(ApiError::bad_request(&format!("scripts holds 1 to {RUN_MAX_SCRIPTS} entries")));
    }
    if b.scripts.iter().any(|s| s.source.len() > crate::script::MAX_SOURCE_BYTES) {
        return Err(ApiError::bad_request("script source too large"));
    }
    if b.time.iter().any(|t| !t.is_finite()) {
        return Err(ApiError::bad_request("time holds epoch milliseconds"));
    }
    let bars = std::sync::Arc::new(crate::script::Cols {
        time: b.time.iter().map(|t| *t as i64).collect(),
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
    });
    let warm = std::sync::Arc::new(stored_warmup(&state, &b.provider, &b.asset_type, b.ticker.trim(), &b.timeframe, bars.time[0]).await?);
    let mut parts = Vec::with_capacity(b.scripts.len());
    for s in b.scripts {
        let answer = crate::script::run_on_chart(s.source, s.ctx, warm.clone(), bars.clone()).await;
        parts.push(format!("{{\"key\":{},\"answer\":{answer}}}", serde_json::to_string(&s.key).unwrap_or_default()));
    }
    let body = format!("{{\"results\":[{}]}}", parts.join(","));
    Ok(([(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// The stored bars strictly before `first_ms` (the newest `FIRST_BAR_WINDOW`), none when the
/// instrument is not stored: the warmup the chart's own host would have read.
async fn stored_warmup(
    state: &AppState,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
    first_ms: i64,
) -> Result<crate::script::Cols, ApiError> {
    let Some(ds) = hist_store::find_dataset(&state.pool, provider, asset_type, ticker, timeframe).await? else {
        return Ok(Default::default());
    };
    let Ok(first) = time::OffsetDateTime::from_unix_timestamp_nanos(first_ms as i128 * 1_000_000) else {
        return Ok(Default::default());
    };
    let window = otw_script::host::FIRST_BAR_WINDOW as i64 + 1;
    let rows = hist_store::read_bars(&state.pool, ds.id, None, Some(first), window).await?;
    let w = crate::backtest_api::warm_bars(rows, first);
    Ok(crate::script::Cols { time: w.time_ms, open: w.open, high: w.high, low: w.low, close: w.close, volume: w.volume })
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
    // Every item is mapped before anything is written: the Watchlists module files an item
    // under its own reconciliation providers (CoinGecko, Yahoo) and quotes it through a
    // Historical Data connector when one is pinned. A chart item is pinned to the connector it
    // is drawn from, under its own ticker, so the watchlist quotes exactly what the chart shows.
    // An item the module cannot hold (an asset class it does not file, no connector) is
    // skipped and named, never guessed into another instrument.
    struct Promoted {
        class: &'static str,
        provider: &'static str,
        ticker: String,
        label: String,
        source: Uuid,
    }
    let mut mapped: Vec<Promoted> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for it in &items {
        let ticker = it.get("ticker").and_then(Value::as_str).unwrap_or("").trim();
        if ticker.is_empty() {
            continue;
        }
        let provider = it.get("provider").and_then(Value::as_str).unwrap_or("").trim();
        let asset_type = it.get("asset_type").and_then(Value::as_str).unwrap_or("").trim();
        let label = it.get("name").and_then(Value::as_str).unwrap_or(ticker).to_string();
        let class = asset_class_of(asset_type);
        let reason = if !matches!(class, "crypto" | "stock" | "etf") {
            Some(format!("watchlists do not hold {asset_type} instruments"))
        } else {
            None
        };
        let connector = match it.get("connector_id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) {
            Some(id) => Some(id),
            None => otw_store::connectors::default_for(&state.pool, provider).await?.map(|c| c.id),
        };
        match (reason, connector) {
            (Some(why), _) => skipped.push(json!({ "ticker": ticker, "reason": why })),
            (None, None) => skipped.push(json!({ "ticker": ticker, "reason": format!("no {provider} connector to quote it through") })),
            (None, Some(source)) => mapped.push(Promoted {
                class,
                provider: if class == "crypto" { "coingecko" } else { "yahoo" },
                ticker: ticker.to_string(),
                label,
                source,
            }),
        }
    }
    let wl = otw_store::watchlists::create_watchlist(&state.pool, name, "").await?;
    for p in &mapped {
        let added = async {
            let item = otw_store::watchlists::add_item(&state.pool, wl.id, p.class, p.provider, &p.ticker, &p.ticker, &p.label).await?;
            let source = p.source.to_string();
            otw_store::watchlists::update_item(&state.pool, item.id, None, None, Some(&p.ticker), Some(&source)).await
        }
        .await;
        if let Err(e) = added {
            // No half-made watchlist left behind.
            let _ = otw_store::watchlists::delete_watchlist(&state.pool, wl.id).await;
            return Err(e.into());
        }
    }
    let added = mapped.len();
    Ok(Json(json!({ "watchlist": wl, "added": added, "skipped": skipped })))
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
    /// The series watched, a backtest operand: `{kind:"price", field}` (open, high, low,
    /// close, volume), `{kind:"indicator", indicator, period, fast, slow, mult,
    /// signal_period}`, `{kind:"custom_indicator", id, plot}` (a library row; `plot` names a
    /// script's plot) or `{kind:"metric", metric, period}` (change_pct, rel_volume,
    /// dd_from_high, up_from_low, change_from_start). Required, except for `at_time`.
    lhs: Option<Operand>,
    /// What it is compared with: the same shapes, or `{kind:"const", value}` for a level.
    /// Required, except for `at_time`.
    rhs: Option<Operand>,
    /// Input values of the scripts either side reads, keyed by indicator id; absent = defaults.
    inputs: Option<serde_json::Map<String, Value>>,
    /// `above` and `below` are crossings, not states: an alert placed under the price does
    /// not fire until the price actually comes back through it. `at_time` compares nothing:
    /// it fires once when the clock reaches `fire_at`.
    op: String,
    /// RFC 3339. Past it the alert is no longer watched and is disabled. Absent = never.
    expires_at: Option<String>,
    /// RFC 3339, the instant an `at_time` alert fires at. Ignored for every other op.
    fire_at: Option<String>,
    /// Notification channels; empty = every channel the chart is granted.
    channels: Option<Vec<Uuid>>,
    /// `app` (live feed, only while the app is open), `background` (closed bars, polled by
    /// the server, default) or `background_live` (live feed held by the server). The live
    /// modes need a provider that streams this asset type and timeframe.
    mode: Option<String>,
    /// `once` (default, then disables itself), `every` (each crossing), `bar_close` (closed
    /// bars only), `cooldown` (at most once per `cooldown_secs`) or `session` (at most once
    /// per occurrence of `session_id`, and only inside it).
    frequency: Option<String>,
    cooldown_secs: Option<i32>,
    /// A market session from `/api/market-sessions`, for the `session` frequency.
    session_id: Option<Uuid>,
    enabled: Option<bool>,
}

const ALERT_OPS: &[&str] = &["above", "below", "crosses", "close_above", "close_below", "at_time"];
const ALERT_MODES: &[&str] = &["app", "background", "background_live"];
const ALERT_FREQUENCIES: &[&str] = &["once", "every", "bar_close", "cooldown", "session"];

/// An RFC 3339 field, blank = absent.
fn rfc3339(v: Option<&str>, field: &str) -> Result<Option<time::OffsetDateTime>, ApiError> {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
            .map(Some)
            .map_err(|_| ApiError::bad_request(&format!("{field} must be an RFC 3339 date and time"))),
    }
}

impl AlertBody {
    fn at_time(&self) -> bool {
        self.op == "at_time"
    }

    /// The two operands. A time alert compares nothing and stores close against 0, so every
    /// reader of the row keeps a valid pair.
    fn pair(&self) -> Result<(Operand, Operand), ApiError> {
        if self.at_time() {
            return Ok((Operand::Price { field: "close".into() }, Operand::Const { value: 0.0 }));
        }
        match (&self.lhs, &self.rhs) {
            (Some(l), Some(r)) => Ok((l.clone(), r.clone())),
            _ => Err(ApiError::bad_request("lhs and rhs are required")),
        }
    }

    /// Validate the vocabulary and hand back the row to write, plus the JSON the input
    /// borrows (it has to outlive the call, so the caller owns it).
    fn parts(&self) -> Result<(store::AlertInput<'_>, AlertJson), ApiError> {
        if !ALERT_OPS.contains(&self.op.as_str()) {
            return Err(ApiError::bad_request(
                "op must be above, below, crosses, close_above, close_below or at_time",
            ));
        }
        let (lhs, rhs) = self.pair()?;
        crate::histviz_alerts::check_pair(&lhs, &rhs).map_err(|e| ApiError::bad_request(&e))?;
        let enabled = self.enabled.unwrap_or(true);
        // A time alert is its own expiry: it fires once at `fire_at`, polled on the minute.
        let at_time = self.at_time();
        let expires_at = if at_time { None } else { rfc3339(self.expires_at.as_deref(), "expires_at")? };
        let fire_at = if at_time {
            let t = rfc3339(self.fire_at.as_deref(), "fire_at")?
                .ok_or_else(|| ApiError::bad_request("a time alert needs fire_at"))?;
            if enabled && t <= time::OffsetDateTime::now_utc() {
                return Err(ApiError::bad_request("this time has passed: pick a later one to arm the alert"));
            }
            Some(t)
        } else {
            None
        };
        // Arming an alert that can no longer fire would be a promise the list cannot keep.
        if enabled && expires_at.is_some_and(|t| t <= time::OffsetDateTime::now_utc()) {
            return Err(ApiError::bad_request("this alert expired: move its expiry date to arm it again"));
        }
        let mode = if at_time { "background" } else { self.mode.as_deref().unwrap_or("background") };
        if !ALERT_MODES.contains(&mode) {
            return Err(ApiError::bad_request("mode must be app, background or background_live"));
        }
        let frequency = if at_time { "once" } else { self.frequency.as_deref().unwrap_or("once") };
        if !ALERT_FREQUENCIES.contains(&frequency) {
            return Err(ApiError::bad_request(
                "frequency must be once, every, bar_close, cooldown or session",
            ));
        }
        let cooldown_secs = self.cooldown_secs.unwrap_or(0).max(0);
        if frequency == "cooldown" && cooldown_secs == 0 {
            return Err(ApiError::bad_request("a cooldown alert needs a cooldown longer than 0 s"));
        }
        if frequency == "session" && self.session_id.is_none() {
            return Err(ApiError::bad_request("a once-per-session alert needs a session"));
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
        // A live mode on a provider that cannot stream this is refused now, with the reason,
        // rather than left to fail quietly in the supervisor.
        if mode != "background" {
            if let Some(why) = crate::live::stream_refusal(
                self.provider.trim(),
                self.asset_type.trim(),
                self.timeframe.trim(),
            ) {
                return Err(ApiError::bad_request(&format!("live alert refused: {why}")));
            }
        }
        let json = AlertJson {
            channels: json!(self.channels.clone().unwrap_or_default()),
            lhs: json!(lhs),
            rhs: json!(rhs),
            inputs: Value::Object(self.inputs.clone().unwrap_or_default()),
        };
        Ok((
            store::AlertInput {
                name: self.name.as_deref().unwrap_or("").trim(),
                provider: self.provider.trim(),
                asset_type: self.asset_type.trim(),
                ticker: self.ticker.trim(),
                timeframe: self.timeframe.trim(),
                connector_id: self.connector_id,
                lhs: &EMPTY_ARRAY,
                rhs: &EMPTY_ARRAY,
                inputs: &EMPTY_ARRAY,
                op: &self.op,
                channels: &EMPTY_ARRAY,
                mode,
                frequency,
                cooldown_secs,
                session_id: if frequency == "session" { self.session_id } else { None },
                enabled,
                expires_at,
                fire_at,
            },
            json,
        ))
    }

    /// The library rows either side names exist and can drive an alert (a script plot that
    /// repaints cannot).
    async fn check_library(&self, pool: &sqlx::PgPool) -> Result<(), ApiError> {
        let inputs = Value::Object(self.inputs.clone().unwrap_or_default());
        let (lhs, rhs) = self.pair()?;
        crate::histviz_alerts::Probe::resolve(pool, lhs, rhs, &inputs)
            .await
            .map(|_| ())
            .map_err(|e| ApiError::bad_request(&e))
    }
}

/// The JSON columns of one alert write, owned by the handler while the input borrows them.
struct AlertJson {
    channels: Value,
    lhs: Value,
    rhs: Value,
    inputs: Value,
}

impl AlertJson {
    fn fill<'a>(&'a self, input: &mut store::AlertInput<'a>) {
        input.channels = &self.channels;
        input.lhs = &self.lhs;
        input.rhs = &self.rhs;
        input.inputs = &self.inputs;
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AlertQuery {
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
    let (mut input, json) = b.parts()?;
    json.fill(&mut input);
    b.check_library(&state.pool).await?;
    let a = store::create_alert(&state.pool, input).await?;
    state.live_alerts.poke();
    Ok(Json(json!(a)))
}

async fn update_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AlertBody>,
) -> Result<Json<Value>, ApiError> {
    let (mut input, json) = b.parts()?;
    json.fill(&mut input);
    b.check_library(&state.pool).await?;
    let a = store::update_alert(&state.pool, id, input)
        .await?
        .ok_or_else(|| ApiError::not_found("alert"))?;
    state.live_alerts.poke();
    Ok(Json(json!(a)))
}

/// POST /api/histviz/alerts/pause-all , disable every armed alert at once.
async fn pause_all_alerts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let paused = store::pause_all_alerts(&state.pool).await?;
    state.live_alerts.poke();
    Ok(Json(json!({ "paused": paused })))
}

/// POST /api/histviz/alerts/presence , the app is open in a browser. `app` alerts hold their
/// live feed while these keep coming.
async fn alert_presence(State(state): State<AppState>) -> Json<Value> {
    state.live_alerts.touch();
    Json(json!({ "ok": true }))
}

async fn delete_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_alert(&state.pool, id).await? {
        return Err(ApiError::not_found("alert"));
    }
    state.live_alerts.poke();
    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(v: Value) -> AlertBody {
        serde_json::from_value(v).unwrap()
    }

    fn base(op: &str) -> Value {
        json!({ "provider": "binance", "asset_type": "crypto", "ticker": "BTCUSDT", "timeframe": "1h", "op": op })
    }

    #[test]
    fn a_time_alert_needs_no_series_and_fires_once_in_the_background() {
        let mut v = base("at_time");
        v["fire_at"] = json!("2999-01-01T12:00:00Z");
        v["mode"] = json!("background_live");
        v["frequency"] = json!("every");
        v["expires_at"] = json!("2998-01-01T00:00:00Z");
        let b = body(v);
        let Ok((input, json)) = b.parts() else { panic!("refused") };
        assert_eq!(input.op, "at_time");
        assert_eq!(input.mode, "background");
        assert_eq!(input.frequency, "once");
        assert!(input.expires_at.is_none());
        assert_eq!(input.fire_at.unwrap().year(), 2999);
        assert_eq!(json.lhs, json!({ "kind": "price", "field": "close" }));
    }

    #[test]
    fn a_time_alert_refuses_a_missing_or_past_time() {
        assert!(body(base("at_time")).parts().is_err());
        let mut v = base("at_time");
        v["fire_at"] = json!("2001-01-01T00:00:00Z");
        assert!(body(v.clone()).parts().is_err());
        // Paused, a past time is just the record of one that fired.
        v["enabled"] = json!(false);
        assert!(body(v).parts().is_ok());
    }

    #[test]
    fn a_series_alert_still_needs_both_sides() {
        let mut v = base("above");
        v["lhs"] = json!({ "kind": "price", "field": "close" });
        assert!(body(v.clone()).parts().is_err());
        v["rhs"] = json!({ "kind": "const", "value": 100.0 });
        let b = body(v);
        let Ok((input, _)) = b.parts() else { panic!("refused") };
        assert!(input.fire_at.is_none());
    }
}
