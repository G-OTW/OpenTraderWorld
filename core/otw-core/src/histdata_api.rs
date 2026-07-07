//! HTTP API for the Historical Data module.
//!
//! - `GET  /api/histdata/providers`            capability matrix + which creds are set
//! - `POST /api/histdata/providers/{p}/secrets`  set a provider credential (write-only)
//! - `DELETE /api/histdata/providers/{p}/secrets/{name}`
//! - `POST /api/histdata/downloads`            queue a download job
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
    routing::{delete, get, post},
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
        .route("/api/histdata/providers/{provider}/secrets", post(set_secret))
        .route(
            "/api/histdata/providers/{provider}/secrets/{name}",
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

// ── Providers / credentials ────────────────────────────────────────────────────

async fn providers(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut out = Vec::new();
    for cap in histdata::capabilities() {
        let names = store::list_cred_names(&state.pool, cap.provider).await?;
        out.push(json!({
            "provider": cap.provider,
            "label": cap.label,
            "website": cap.website,
            "docs_url": cap.docs_url,
            "rate_limit": cap.rate_limit,
            "paid": cap.paid,
            "required_secrets": cap.required_secrets,
            "set_secrets": names,
            "asset_types": cap.asset_types,
            "timeframes": cap.timeframes,
            "adjusted": cap.adjusted,
        }));
    }
    Ok(Json(json!({ "providers": out })))
}

#[derive(Deserialize)]
struct SecretBody {
    name: String,
    value: String,
}

async fn set_secret(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(body): Json<SecretBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() || body.value.is_empty() {
        return Err(ApiError::bad_request("secret name and value are required"));
    }
    histdata::connector_for(&provider).map_err(|_| ApiError::not_found("unknown provider"))?;
    store::set_cred(&state.pool, &state.cipher, &provider, body.name.trim(), &body.value).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_secret(
    State(state): State<AppState>,
    Path((provider, name)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_cred(&state.pool, &provider, &name).await? {
        return Err(ApiError::not_found("secret not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Downloads ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DownloadBody {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// RFC3339 inclusive start.
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
    // Enforce the capability matrix server-side (the UI greys these out, but never trust it).
    histdata::validate_request(&b.provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let from = parse_rfc3339(&b.from, "from")?;
    let to = parse_rfc3339(&b.to, "to")?;
    if from >= to {
        return Err(ApiError::bad_request("'from' must be before 'to'"));
    }

    let dataset_id =
        store::upsert_dataset(&state.pool, &b.provider, &b.asset_type, ticker, &b.timeframe).await?;
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id,
            provider: &b.provider,
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
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id: id,
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
