//! HTTP API for the Historical Data module.
//!
//! - `GET  /api/histdata/providers`            capability matrix (drives the connector form)
//! - `GET  /api/histdata/connectors`           named connectors + creds status + quota usage
//! - `POST /api/histdata/connectors`           create a connector (provider, name, limit?)
//! - `PATCH/DELETE /api/histdata/connectors/{id}`  rename / set limit / remove
//! - `POST /api/histdata/connectors/{id}/secrets`  set a credential (write-only)
//! - `DELETE /api/histdata/connectors/{id}/secrets/{name}`
//! - `POST /api/histdata/downloads`            queue a download job (connector_id or provider)
//! - `GET  /api/histdata/jobs`                 recent jobs (download-page progress)
//! - `GET  /api/histdata/datasets`             catalog (management page)
//! - `POST /api/histdata/datasets/{id}/append` gap-fill a dataset (max(ts)→now)
//! - `GET  /api/histdata/datasets/{id}/bars`   OHLCV JSON for the visualization module
//! - `GET  /api/histdata/datasets/{id}/export` CSV download
//! - `DELETE /api/histdata/datasets/{id}`

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::histdata;
use crate::{ApiError, AppState};
use otw_store::histdata as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/histdata/providers", get(providers))
        .route("/api/histdata/connectors", get(connectors).post(create_connector))
        .route(
            "/api/histdata/connectors/{id}",
            patch(update_connector).delete(remove_connector),
        )
        .route("/api/histdata/connectors/{id}/secrets", post(set_secret))
        .route(
            "/api/histdata/connectors/{id}/secrets/{name}",
            delete(delete_secret),
        )
        .route("/api/histdata/downloads", post(start_download))
        .route("/api/histdata/jobs", get(jobs))
        .route("/api/histdata/datasets", get(datasets))
        .route("/api/histdata/datasets/{id}/append", post(append))
        .route("/api/histdata/datasets/{id}/bars", get(bars))
        .route("/api/histdata/datasets/{id}/export", get(export))
        .route("/api/histdata/datasets/{id}", delete(remove_dataset))
        .route(
            "/api/histviz/chart-settings",
            get(get_chart_settings).put(set_chart_settings),
        )
}

// ── Visualization chart settings (single global JSON blob) ──────────────────────

const CHART_SETTINGS_KEY: &str = "histviz.chart_settings";

/// Saved chart settings, or `null` if never customized (client applies its defaults).
async fn get_chart_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let raw = otw_store::settings::get(&state.pool, CHART_SETTINGS_KEY)
        .await
        .map_err(ApiError::from)?;
    let out = match raw {
        Some(s) if !s.trim().is_empty() => serde_json::from_str::<Value>(&s).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    Ok(Json(out))
}

/// Replace the saved chart settings with the posted JSON object.
async fn set_chart_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let serialized = serde_json::to_string(&body)
        .map_err(|e| ApiError::bad_request(&format!("invalid settings: {e}")))?;
    otw_store::settings::set(&state.pool, CHART_SETTINGS_KEY, &serialized)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(body))
}

// ── Providers / connectors / credentials ─────────────────────────────────────────

/// Capability matrix only — connector-independent facts about each supported provider.
/// The "add connector" form and MCP clients read this.
async fn providers(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let out: Vec<Value> = histdata::capabilities()
        .into_iter()
        .map(|cap| {
            json!({
                "provider": cap.provider,
                "label": cap.label,
                "website": cap.website,
                "docs_url": cap.docs_url,
                "rate_limit": cap.rate_limit,
                "required_secrets": cap.required_secrets,
                "asset_types": cap.asset_types,
                "timeframes": cap.timeframes,
                "adjusted": cap.adjusted,
            })
        })
        .collect();
    Ok(Json(json!({ "providers": out })))
}

/// The api_quota scope of a connector.
fn quota_scope(id: Uuid) -> String {
    format!("histconn:{id}")
}

/// A connector list/create call's owning module. Historical Data and Watchlists keep
/// separate connector namespaces over the same mechanics.
fn valid_scope(scope: Option<&str>) -> Result<&str, ApiError> {
    match scope.unwrap_or("histdata") {
        s @ ("histdata" | "watchlists") => Ok(s),
        _ => Err(ApiError::bad_request("scope must be 'histdata' or 'watchlists'")),
    }
}

#[derive(Deserialize)]
struct ConnectorsQuery {
    scope: Option<String>,
}

/// Connectors with capability facts, credential status and current quota usage merged
/// in — one call drives both the settings page and the download picker.
async fn connectors(
    State(state): State<AppState>,
    Query(q): Query<ConnectorsQuery>,
) -> Result<Json<Value>, ApiError> {
    let scope = valid_scope(q.scope.as_deref())?;
    let rows = store::list_connectors(&state.pool, scope).await?;
    let quotas = otw_store::api_quota::list_prefixed(&state.pool, "histconn:").await?;
    let mut out = Vec::with_capacity(rows.len());
    for c in rows {
        // A connector whose provider was removed from the build is skipped, not fatal.
        let Ok(cap) = histdata::connector_for(&c.provider).map(|k| k.capability()) else {
            continue;
        };
        let names = store::list_cred_names(&state.pool, c.id).await?;
        let secrets: Vec<Value> = store::list_cred_meta(&state.pool, c.id)
            .await?
            .into_iter()
            .map(|(name, vault_item_id)| json!({ "name": name, "vault_item_id": vault_item_id }))
            .collect();
        let quota = quotas.iter().find(|q| q.scope == quota_scope(c.id));
        out.push(json!({
            "id": c.id,
            "provider": c.provider,
            "name": c.name,
            "label": cap.label,
            "website": cap.website,
            "docs_url": cap.docs_url,
            "rate_limit": cap.rate_limit,
            "required_secrets": cap.required_secrets,
            "set_secrets": names,
            "secrets": secrets,
            "asset_types": cap.asset_types,
            "timeframes": cap.timeframes,
            "adjusted": cap.adjusted,
            "quota": quota,
        }));
    }
    Ok(Json(json!({ "connectors": out })))
}

/// Optional request limit on a connector: `enabled` toggles tracking, `max_requests`
/// NULL/absent = unlimited (still tracked), `period` per api_quota::PERIODS.
#[derive(Deserialize)]
struct LimitBody {
    enabled: bool,
    max_requests: Option<i64>,
    #[serde(default)]
    period: String,
}

/// Apply a limit declaration to a connector's quota scope.
async fn apply_limit(state: &AppState, id: Uuid, limit: &LimitBody) -> Result<(), ApiError> {
    if !limit.enabled {
        otw_store::api_quota::remove(&state.pool, &quota_scope(id)).await?;
        return Ok(());
    }
    if !otw_store::api_quota::valid_period(&limit.period) {
        return Err(ApiError::bad_request("period must be minute|hour|day|week|month"));
    }
    if limit.max_requests.is_some_and(|n| n < 1) {
        return Err(ApiError::bad_request("max_requests must be at least 1"));
    }
    otw_store::api_quota::set(&state.pool, &quota_scope(id), limit.max_requests, &limit.period)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct CreateConnector {
    provider: String,
    name: String,
    limit: Option<LimitBody>,
    scope: Option<String>,
}

async fn create_connector(
    State(state): State<AppState>,
    Json(body): Json<CreateConnector>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("connector name is required"));
    }
    let scope = valid_scope(body.scope.as_deref())?;
    histdata::connector_for(&body.provider).map_err(|_| ApiError::not_found("unknown provider"))?;
    let row = store::create_connector(&state.pool, &body.provider, name, scope)
        .await
        .map_err(|_| ApiError::bad_request("a connector with this name already exists"))?;
    if let Some(limit) = &body.limit {
        apply_limit(&state, row.id, limit).await?;
    }
    Ok(Json(json!({ "connector": row })))
}

#[derive(Deserialize)]
struct PatchConnector {
    name: Option<String>,
    limit: Option<LimitBody>,
}

async fn update_connector(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchConnector>,
) -> Result<Json<Value>, ApiError> {
    if store::get_connector(&state.pool, id).await?.is_none() {
        return Err(ApiError::not_found("connector not found"));
    }
    if let Some(name) = &body.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("connector name is required"));
        }
        store::rename_connector(&state.pool, id, name)
            .await
            .map_err(|_| ApiError::bad_request("a connector with this name already exists"))?;
    }
    if let Some(limit) = &body.limit {
        apply_limit(&state, id, limit).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove_connector(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_connector(&state.pool, id).await? {
        return Err(ApiError::not_found("connector not found"));
    }
    otw_store::api_quota::remove(&state.pool, &quota_scope(id)).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SecretBody {
    name: String,
    /// Plaintext value (write-only), or empty when plugging a vault item instead.
    #[serde(default)]
    value: String,
    /// Centralized vault item to resolve this credential from (wins over `value`).
    #[serde(default)]
    vault_item_id: Option<Uuid>,
}

async fn set_secret(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SecretBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("secret name is required"));
    }
    let conn = store::get_connector(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("connector not found"))?;
    match body.vault_item_id {
        Some(item) => {
            store::set_cred_ref(&state.pool, id, &conn.provider, body.name.trim(), item).await?
        }
        None => {
            if body.value.is_empty() {
                return Err(ApiError::bad_request("secret value is required"));
            }
            store::set_cred(&state.pool, &state.cipher, id, &conn.provider, body.name.trim(), &body.value)
                .await?;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_secret(
    State(state): State<AppState>,
    Path((id, name)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_cred(&state.pool, id, &name).await? {
        return Err(ApiError::not_found("secret not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Downloads ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DownloadBody {
    /// The connector to download through (preferred). When absent, `provider` must be
    /// set and the provider's default connector is used (MCP / legacy clients).
    connector_id: Option<Uuid>,
    /// Provider id, e.g. "binance" — see /api/histdata/providers.
    provider: Option<String>,
    /// One of the provider's asset_types, e.g. "crypto" — see /api/histdata/providers.
    asset_type: String,
    /// Symbol in the provider's format, e.g. "BTCUSDT" for binance.
    ticker: String,
    /// One of the provider's timeframes, e.g. "1h".
    timeframe: String,
    /// RFC3339 inclusive start, e.g. "2024-07-19T00:00:00Z".
    from: String,
    /// RFC3339 exclusive end.
    to: String,
}

fn parse_rfc3339(s: &str, field: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(s.trim(), &Rfc3339)
        .map_err(|_| ApiError::bad_request(&format!("invalid {field} (need RFC3339)")))
}

async fn start_download(
    State(state): State<AppState>,
    Json(b): Json<DownloadBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    // Resolve the connector: explicit id wins; a bare provider (MCP / legacy clients)
    // maps to that provider's default connector when one exists.
    let (connector_id, provider) = match (b.connector_id, b.provider.as_deref()) {
        (Some(id), _) => {
            let c = store::get_connector(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            (Some(id), c.provider)
        }
        (None, Some(p)) => {
            let c = store::default_connector_for(&state.pool, p).await?;
            (c.map(|c| c.id), p.to_string())
        }
        (None, None) => {
            return Err(ApiError::bad_request("connector_id or provider is required"));
        }
    };
    // Enforce the capability matrix server-side (the UI greys these out, but never trust it).
    histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let from = parse_rfc3339(&b.from, "from")?;
    let to = parse_rfc3339(&b.to, "to")?;
    if from >= to {
        return Err(ApiError::bad_request("'from' must be before 'to'"));
    }

    let dataset_id =
        store::upsert_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe).await?;
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id,
            connector_id,
            provider: &provider,
            asset_type: &b.asset_type,
            ticker,
            timeframe: &b.timeframe,
            range_from: from,
            range_to: to,
            kind: "download",
        },
    )
    .await?;
    Ok(Json(json!({ "job_id": job_id, "dataset_id": dataset_id })))
}

async fn jobs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jobs = store::list_jobs(&state.pool, 50).await?;
    Ok(Json(json!({ "jobs": jobs })))
}

// ── Datasets (management page) ───────────────────────────────────────────────────

async fn datasets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let datasets = store::list_datasets(&state.pool).await?;
    Ok(Json(json!({ "datasets": datasets })))
}

/// Append/gap-fill: queue a download from the dataset's latest bar up to now.
async fn append(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    // Start one timeframe after the last bar (or 30d back if empty).
    let step = histdata::timeframe_secs(&ds.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let from = match ds.range_to {
        Some(t) => t + time::Duration::seconds(step),
        None => OffsetDateTime::now_utc() - time::Duration::days(30),
    };
    let to = OffsetDateTime::now_utc();
    if from >= to {
        return Ok(Json(json!({ "ok": true, "skipped": "already current" })));
    }
    // Datasets are provider-keyed, not connector-keyed; append through the default one.
    let connector_id = store::default_connector_for(&state.pool, &ds.provider)
        .await?
        .map(|c| c.id);
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id: id,
            connector_id,
            provider: &ds.provider,
            asset_type: &ds.asset_type,
            ticker: &ds.ticker,
            timeframe: &ds.timeframe,
            range_from: from,
            range_to: to,
            kind: "append",
        },
    )
    .await?;
    Ok(Json(json!({ "job_id": job_id })))
}

#[derive(Deserialize)]
struct BarsQuery {
    /// RFC3339 inclusive lower bound (optional).
    from: Option<String>,
    /// RFC3339 inclusive upper bound (optional).
    to: Option<String>,
    /// Max bars to return; clamped to [1, 50000]. Defaults to 5000.
    limit: Option<i64>,
}

/// OHLCV for the visualization module. Returns the dataset header plus parallel arrays
/// (ts as RFC3339 strings, o/h/l/c/v as numbers) — compact and ECharts-ready.
async fn bars(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<BarsQuery>,
) -> Result<Json<Value>, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let from = q.from.as_deref().map(|s| parse_rfc3339(s, "from")).transpose()?;
    let to = q.to.as_deref().map(|s| parse_rfc3339(s, "to")).transpose()?;
    let limit = q.limit.unwrap_or(5000).clamp(1, 50_000);

    let rows = store::read_bars(&state.pool, id, from, to, limit).await?;
    let mut ts = Vec::with_capacity(rows.len());
    let (mut o, mut h, mut l, mut c, mut v) = (
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
    );
    for b in &rows {
        ts.push(b.ts.format(&Rfc3339).unwrap_or_default());
        o.push(b.open);
        h.push(b.high);
        l.push(b.low);
        c.push(b.close);
        v.push(b.volume);
    }
    Ok(Json(json!({
        "id": ds.id,
        "provider": ds.provider,
        "asset_type": ds.asset_type,
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "count": rows.len(),
        "ts": ts, "o": o, "h": h, "l": l, "c": c, "v": v,
    })))
}

async fn export(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let rows = store::export_rows(&state.pool, id).await?;

    let mut csv = String::from("ts,open,high,low,close,volume,adj_open,adj_high,adj_low,adj_close\n");
    let opt = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
    for b in &rows {
        let ts = b.ts.format(&Rfc3339).unwrap_or_default();
        csv.push_str(&format!(
            "{ts},{},{},{},{},{},{},{},{},{}\n",
            b.open, b.high, b.low, b.close, b.volume,
            opt(b.adj_open), opt(b.adj_high), opt(b.adj_low), opt(b.adj_close),
        ));
    }
    let filename = format!("{}_{}_{}.csv", ds.provider, ds.ticker, ds.timeframe);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    )
        .into_response())
}

async fn remove_dataset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_dataset(&state.pool, id).await? {
        return Err(ApiError::not_found("dataset not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
