//! HTTP API for the Historical Data module.
//!
//! Provider connectors themselves live in the data broker (`connectors_api`); this module
//! spends them: queueing downloads, serving stored bars, and the ad-hoc preview the chart
//! uses to look at an instrument before deciding to store it.
//!
//! - `POST /api/histdata/downloads`            queue a download job (connector_id or provider)
//! - `GET  /api/histdata/jobs`                 recent jobs (download-page progress)
//! - `GET  /api/histdata/datasets`             catalog (management page)
//! - `POST /api/histdata/datasets/{id}/append` gap-fill a dataset (max(ts)→now)
//! - `GET  /api/histdata/datasets/{id}/bars`   OHLCV JSON for the visualization module
//! - `GET  /api/histdata/datasets/{id}/export` CSV download
//! - `DELETE /api/histdata/datasets/{id}`
//! - `POST /api/histdata/preview`              fetch bars live, store nothing
//! - `POST /api/histdata/preview/save`         store what the preview showed (consolidating)
//! - `GET  /api/histdata/symbols`              instrument lookup across granted connectors
//! - `POST /api/histviz/series`                one chart slice: stored bars + gap-filled fetch
//! - `GET  /api/histviz/stream`                SSE live bars for an instrument (writes nothing)

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::histdata;
use crate::live::{self, hub::LiveTarget};
use crate::{ApiError, AppState};
use otw_store::connectors as conn_store;
use otw_store::histdata as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/histdata/downloads", post(start_download))
        .route("/api/histdata/preview", post(preview))
        .route("/api/histdata/preview/save", post(save_preview))
        .route("/api/histdata/symbols", get(symbols))
        .route("/api/histviz/series", post(series))
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
        // Live market data: the SSE stream of forming/closed bars for one instrument.
        // Addressed by coordinates, so watching a symbol never creates anything.
        .route("/api/histviz/stream", get(stream_bars))
}

// ── Live market data ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StreamQuery {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
}

/// GET /api/histviz/stream?provider=&asset_type=&ticker=&timeframe= — Server-Sent Events for
/// an *instrument*, stored or not. Emits one `snapshot` event with the forming bar on connect
/// (if any), then a `bar` event per live update (`closed` tells the client whether to replace
/// the last candle or append a new one). The WS feed is ref-counted: it starts on the first
/// viewer and tears down shortly after the last leaves.
///
/// Watching writes nothing. If these coordinates already have a dataset, the same feed also
/// records its closed bars into it — saving an instrument is what turns recording on.
async fn stream_bars(
    State(state): State<AppState>,
    Query(q): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let ticker = q.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    if !live::stream_capable(&q.provider) {
        return Err(ApiError::bad_request(&format!(
            "{} has no live feed",
            q.provider
        )));
    }
    histdata::validate_request(&q.provider, &q.asset_type, &q.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let dataset_id = store::find_dataset(&state.pool, &q.provider, &q.asset_type, ticker, &q.timeframe)
        .await?
        .map(|d| d.id);
    let target = LiveTarget {
        dataset_id,
        provider: q.provider,
        asset_type: q.asset_type,
        ticker: ticker.to_string(),
        timeframe: q.timeframe,
    };
    let live::hub::Subscription { rx, snapshot, guard } = state
        .live
        .subscribe(target)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;

    // Keep the ref-count guard alive for the stream's whole life by moving it into the map
    // closure — dropping the stream drops the guard, releasing the feed.
    let snap = futures::stream::iter(
        snapshot
            .into_iter()
            .map(|e| Ok(sse_event("snapshot", &e))),
    );
    let updates = BroadcastStream::new(rx).filter_map(|m| async move {
        match m {
            Ok(e) => Some(Ok(sse_event("bar", &e))),
            // A lagging viewer's dropped messages are skipped; it resyncs from the snapshot.
            Err(_) => None,
        }
    });
    let stream = snap.chain(updates).map(move |ev| {
        let _keep = &guard;
        ev
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn sse_event(kind: &str, e: &live::LiveEvent) -> Event {
    Event::default()
        .event(kind)
        .data(serde_json::to_string(&e.to_wire()).unwrap_or_default())
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

// ── Downloads ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DownloadBody {
    /// The connector to download through (preferred). When absent, `provider` must be
    /// set and the provider's default connector is used (MCP / legacy clients).
    connector_id: Option<Uuid>,
    /// Provider id, e.g. "binance" — see /api/connectors/providers.
    provider: Option<String>,
    /// One of the provider's asset_types, e.g. "crypto" — see /api/connectors/providers.
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
            let c = conn_store::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            (Some(id), c.provider)
        }
        (None, Some(p)) => {
            let c = conn_store::default_for(&state.pool, p).await?;
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

// ── Preview (look now, store later) ──────────────────────────────────────────────
//
// The chart can open any instrument a connector serves, not only what was downloaded:
// the preview fetches bars through the connector and returns them without writing a
// single row. Saving is a separate, explicit click — it queues an ordinary download job
// over the same window, so the bars land in the normal catalog and consolidate with
// whatever was already stored (`write_bars` upserts on (dataset, ts)).

/// Trailing bars a preview pulls when the client doesn't say, and the ceiling it may ask.
const PREVIEW_DEFAULT_BARS: i64 = 500;
const PREVIEW_MAX_BARS: i64 = 5_000;
/// Cap on provider round-trips for one preview: a connector that pages 200 bars at a time
/// must not turn one click into an unbounded burst against the user's API plan.
const PREVIEW_MAX_CHUNKS: usize = 12;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PreviewBody {
    /// Connector to fetch through (preferred); must be granted to the chart module.
    connector_id: Option<Uuid>,
    /// Provider id, when no connector is named (uses that provider's default connector).
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Trailing bars to fetch (default 500, capped 5000).
    bars: Option<i64>,
}

/// Resolve `connector_id` / `provider` to a (connector, provider) pair, checking that the
/// connector is granted to `module`. Mirrors the download resolution, plus the grant.
async fn resolve_source(
    state: &AppState,
    connector_id: Option<Uuid>,
    provider: Option<&str>,
    module: &str,
) -> Result<(Option<Uuid>, String), ApiError> {
    match (connector_id, provider) {
        (Some(id), _) => {
            let c = conn_store::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            if !c.allows(module) {
                return Err(ApiError::bad_request(&format!(
                    "connector '{}' is not enabled for {module}",
                    c.name
                )));
            }
            Ok((Some(id), c.provider))
        }
        (None, Some(p)) => {
            let c = conn_store::default_for(&state.pool, p).await?;
            Ok((c.map(|c| c.id), p.to_string()))
        }
        (None, None) => Err(ApiError::bad_request("connector_id or provider is required")),
    }
}

/// POST /api/histdata/preview — fetch the trailing window straight from the provider and
/// return it. Nothing is written; `stored` reports whether a dataset for these coordinates
/// already exists, so the UI can offer "save" vs "update".
async fn preview(
    State(state): State<AppState>,
    Json(b): Json<PreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    let cap = histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let want = b.bars.unwrap_or(PREVIEW_DEFAULT_BARS).clamp(1, PREVIEW_MAX_BARS);

    let to = OffsetDateTime::now_utc();
    let from = to - time::Duration::seconds(tf_secs * want);
    let secrets = match connector_id {
        Some(id) => conn_store::load_creds(&state.pool, &state.cipher, id).await?,
        None => Default::default(),
    };
    let connector = histdata::connector_for(&provider)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let step = tf_secs * cap.max_bars_per_req.max(1) as i64;

    let mut rows: Vec<otw_store::histdata::Bar> = Vec::new();
    let mut cursor = from;
    let mut chunks = 0usize;
    while cursor < to && chunks < PREVIEW_MAX_CHUNKS {
        let chunk_to = (cursor + time::Duration::seconds(step)).min(to);
        if let Some(id) = connector_id {
            // Display-only counter; a failed write must not block the fetch.
            let _ = otw_store::api_quota::bump(&state.pool, &crate::connectors_api::quota_scope(id))
                .await;
        }
        let chunk = connector
            .fetch_chunk(
                &state.http,
                &secrets,
                ticker,
                &b.asset_type,
                &b.timeframe,
                cursor,
                chunk_to,
            )
            .await
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        rows.extend(chunk.bars);
        cursor = chunk_to;
        chunks += 1;
    }
    // Providers may overlap pages or ignore the window; normalize to a clean series.
    rows.sort_by_key(|b| b.ts);
    rows.dedup_by_key(|b| b.ts);
    if rows.len() as i64 > want {
        rows.drain(..rows.len() - want as usize);
    }
    if rows.is_empty() {
        return Err(ApiError::bad_request(&format!(
            "{provider} returned no bars for {ticker} ({} {})",
            b.asset_type, b.timeframe
        )));
    }

    let stored = store::find_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe)
        .await?
        .map(|d| json!({ "id": d.id, "bar_count": d.bar_count }));
    let mut out = bars_payload(&rows);
    out["provider"] = json!(provider);
    out["asset_type"] = json!(b.asset_type);
    out["ticker"] = json!(ticker);
    out["timeframe"] = json!(b.timeframe);
    out["stream"] = json!(live::stream_capable(&provider));
    out["stored"] = stored.unwrap_or(Value::Null);
    Ok(Json(out))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SavePreviewBody {
    connector_id: Option<Uuid>,
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Trailing bars to persist — the same window the preview showed. Ignored when an
    /// explicit `from`/`to` pair is given.
    bars: Option<i64>,
    /// Explicit window start (RFC3339) — what the chart has actually loaded.
    from: Option<String>,
    /// Explicit window end (RFC3339). Defaults to now when only `from` is given.
    to: Option<String>,
}

/// POST /api/histdata/preview/save — persist what the chart is showing by queueing the
/// normal download job for that window. Re-downloading server-side (rather than trusting
/// the bars the browser holds) keeps a single write path and consolidates with existing
/// rows through the same upsert every download uses. Either give an explicit `from`/`to`
/// (the loaded window) or a trailing `bars` count.
async fn save_preview(
    State(state): State<AppState>,
    Json(b): Json<SavePreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let to = match b.to.as_deref() {
        Some(s) => parse_rfc3339(s, "to")?,
        None => OffsetDateTime::now_utc(),
    };
    let from = match b.from.as_deref() {
        Some(s) => parse_rfc3339(s, "from")?,
        None => {
            let want = b.bars.unwrap_or(PREVIEW_DEFAULT_BARS).clamp(1, PREVIEW_MAX_BARS);
            to - time::Duration::seconds(tf_secs * want)
        }
    };
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
    Ok(Json(json!({ "dataset_id": dataset_id, "job_id": job_id })))
}

// ── Symbol lookup ────────────────────────────────────────────────────────────────
//
// The chart opens any instrument a connector serves, so the user must be able to *find*
// it: one search box over every connector granted to the chart, or over the subset they
// ticked. Providers without a search endpoint are reported in `notes` rather than
// silently dropped — their symbols are typed by hand.

/// Hits returned per connector, and across all of them.
const SYMBOLS_PER_CONNECTOR: usize = 12;
const SYMBOLS_TOTAL: usize = 60;

#[derive(Deserialize)]
struct SymbolsQuery {
    /// Free text: symbol fragment or name.
    q: Option<String>,
    /// Restrict to one asset type (must be one the connector supports).
    asset_type: Option<String>,
    /// Comma-separated connector ids (the whitelist). Absent = every granted connector.
    connectors: Option<String>,
    limit: Option<usize>,
}

/// GET /api/histdata/symbols — instrument lookup across the connectors the chart may use.
async fn symbols(
    State(state): State<AppState>,
    Query(q): Query<SymbolsQuery>,
) -> Result<Json<Value>, ApiError> {
    let needle = q.q.unwrap_or_default();
    let asset_type = q.asset_type.unwrap_or_default();
    let per = q.limit.unwrap_or(SYMBOLS_PER_CONNECTOR).clamp(1, 50);

    let wanted: Vec<Uuid> = q
        .connectors
        .as_deref()
        .map(|s| s.split(',').filter_map(|p| Uuid::parse_str(p.trim()).ok()).collect())
        .unwrap_or_default();
    let rows = conn_store::list_for_module(&state.pool, "histviz").await?;

    // Mark hits already in the catalog so the UI can say "you have this one".
    let stored: std::collections::HashSet<(String, String)> =
        store::list_datasets(&state.pool)
            .await?
            .into_iter()
            .map(|d| (d.provider, d.ticker.to_ascii_uppercase()))
            .collect();

    let mut results: Vec<Value> = Vec::new();
    let mut notes: Vec<Value> = Vec::new();
    for c in rows {
        if !wanted.is_empty() && !wanted.contains(&c.id) {
            continue;
        }
        let Ok(connector) = histdata::connector_for(&c.provider) else {
            continue;
        };
        let cap = connector.capability();
        // Asked for an asset type this provider doesn't serve: skip it silently, the
        // connector simply has nothing to say about e.g. options.
        if !asset_type.is_empty() && !cap.asset_types.contains(&asset_type.as_str()) {
            continue;
        }
        if !cap.searchable {
            notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": "unsupported",
                "message": format!("{} has no symbol search — type the symbol directly", cap.label),
            }));
            continue;
        }
        let secrets = conn_store::load_creds(&state.pool, &state.cipher, c.id).await?;
        if let Some(missing) = missing_secret(cap, &secrets) {
            notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": "auth",
                "message": format!("{} needs its '{missing}' credential", cap.label),
            }));
            continue;
        }
        match connector
            .search_symbols(&state.http, &secrets, &needle, &asset_type, per)
            .await
        {
            Ok(hits) => {
                for h in hits {
                    if results.len() >= SYMBOLS_TOTAL {
                        break;
                    }
                    results.push(json!({
                        "connector_id": c.id,
                        "connector": c.name,
                        "provider": c.provider,
                        "label": cap.label,
                        "symbol": h.symbol,
                        "name": h.name,
                        "asset_type": h.asset_type,
                        "exchange": h.exchange,
                        "timeframes": cap.timeframes,
                        "stream": live::stream_capable(&c.provider),
                        "stored": stored.contains(&(c.provider.clone(), h.symbol.to_ascii_uppercase())),
                    }));
                }
            }
            Err(e) => notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": notice_code(&format!("{e:#}")),
                "message": short_reason(&format!("{e:#}")),
            })),
        }
    }
    Ok(Json(json!({ "results": results, "notes": notes })))
}

/// The first required credential a connector is missing, if any.
fn missing_secret<'a>(
    cap: &'a histdata::Capability,
    secrets: &std::collections::HashMap<String, String>,
) -> Option<&'a str> {
    cap.required_secrets
        .iter()
        .copied()
        .find(|n| secrets.get(*n).map(|v| v.trim().is_empty()).unwrap_or(true))
}

/// Trim a provider error down to what a user can act on: the error chain's own words,
/// without the reqwest URL tail that would otherwise leak an API key into the UI.
fn short_reason(msg: &str) -> String {
    let cut = msg.find(" for url (").unwrap_or(msg.len());
    let s = msg[..cut].trim().trim_end_matches(':').trim();
    if s.len() > 240 {
        format!("{}…", &s[..240])
    } else {
        s.to_string()
    }
}

/// Sort a provider/transport failure into a code the UI can phrase for the user. The
/// message itself is always carried through — this only picks the icon and the tone.
fn notice_code(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("429")
        || m.contains("too many requests")
        || m.contains("rate limit")
        || m.contains("ratelimit")
        || m.contains("exceeded")
    {
        "rate_limit"
    } else if m.contains("api key")
        || m.contains("apikey")
        || m.contains("api_key")
        || m.contains("credential")
        || m.contains("unauthorized")
        || m.contains("not_authorized")
        || m.contains("401")
        || m.contains("403")
    {
        "auth"
    } else if m.contains("bad symbol")
        || m.contains("bad pair")
        || m.contains("bad product")
        || m.contains("no data")
        || m.contains("no bars")
        || m.contains("no series")
        || m.contains("404")
    {
        "symbol"
    } else if m.contains("days") || m.contains("history") || m.contains("range") {
        "depth"
    } else {
        "provider"
    }
}

// ── Chart series (stored + provider, gap-filled) ─────────────────────────────────
//
// The chart is not bound to a dataset: it asks for a *window* of an instrument and this
// endpoint answers with whatever it can — bars already in the catalog, plus the missing
// edges fetched through the connector. Nothing is written. One call = one slice; the
// client's "load more" button walks backwards a slice at a time, which is why no fetch
// ever happens without a user gesture.

/// One "load more" click, and the ceiling a client may ask for in a single slice.
const SLICE_BARS: i64 = 1_500;
const SLICE_MAX_BARS: i64 = 5_000;
/// Provider round-trips one slice may spend (a 300-bar pager still fits 1500 bars).
const SLICE_MAX_CHUNKS: usize = 12;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SeriesBody {
    /// Connector to read through (preferred); must be granted to the chart module.
    connector_id: Option<Uuid>,
    /// Provider id, when no connector is named (uses that provider's default connector).
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Exclusive upper bound (RFC3339). Absent = now, i.e. the newest slice.
    to: Option<String>,
    /// Bars in this slice (default 1500, max 5000).
    bars: Option<i64>,
    /// Read stored bars only — never call the provider (no quota spent).
    #[serde(default)]
    stored_only: bool,
}

/// POST /api/histviz/series — a window of an instrument, stored bars first, provider only
/// for what is missing.
async fn series(
    State(state): State<AppState>,
    Json(b): Json<SeriesBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    let cap = histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let want = b.bars.unwrap_or(SLICE_BARS).clamp(1, SLICE_MAX_BARS);

    let to = match b.to.as_deref() {
        Some(s) => parse_rfc3339(s, "to")?,
        None => OffsetDateTime::now_utc(),
    };
    let from = to - time::Duration::seconds(tf_secs * want);

    // What the catalog already holds for this window — free, and never re-downloaded.
    let dataset = store::find_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe)
        .await?;
    // Read a couple of bars more than the slice: `read_bars` keeps the *newest* `limit`
    // rows, and a window trimmed exactly to `want` would look like it were missing its
    // oldest bar — turning a covered window into a one-bar provider call.
    let mut rows: Vec<otw_store::histdata::Bar> = match &dataset {
        Some(d) => store::read_bars(&state.pool, d.id, Some(from), Some(to), want + 2).await?,
        None => Vec::new(),
    };
    let stored_here = rows.len();

    // The holes to ask the provider for: the stretch before the oldest stored bar and the
    // stretch after the newest. One bar of slack keeps a boundary bar from being refetched.
    let step = time::Duration::seconds(tf_secs);
    let mut gaps: Vec<(OffsetDateTime, OffsetDateTime)> = Vec::new();
    let mut asked_older = false;
    match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => {
            if first.ts - from >= step {
                gaps.push((from, first.ts));
                asked_older = true;
            }
            if to - last.ts > step {
                gaps.push((last.ts + step, to));
            }
        }
        _ => {
            gaps.push((from, to));
            asked_older = true;
        }
    }

    let mut notice: Option<Value> = None;
    let mut fetched = 0usize; // bars the provider served *inside* the window
    let mut served = 0usize; // bars it served at all (see the Kraken note below)
    let mut got_older = false;

    if b.stored_only {
        gaps.clear();
        asked_older = false;
    }
    if !gaps.is_empty() {
        let secrets = match connector_id {
            Some(id) => conn_store::load_creds(&state.pool, &state.cipher, id).await?,
            None => Default::default(),
        };
        // Refuse before spending a request when the reason is already knowable: no key,
        // or a limit the user themselves declared on this connector.
        if let Some(missing) = missing_secret(cap, &secrets) {
            notice = Some(json!({
                "code": "auth",
                "message": format!("{} needs its '{missing}' credential — set it on the connector", cap.label),
            }));
        } else if let Some(q) = quota_exhausted(&state, connector_id).await {
            notice = Some(q);
        } else {
            let connector = histdata::connector_for(&provider)
                .map_err(|e| ApiError::bad_request(&e.to_string()))?;
            let page = time::Duration::seconds(tf_secs * cap.max_bars_per_req.max(1) as i64);
            let mut chunks = 0usize;
            'gaps: for (g_from, g_to) in gaps {
                let older = g_from <= from;
                let mut cursor = g_from;
                while cursor < g_to {
                    if chunks >= SLICE_MAX_CHUNKS {
                        break 'gaps;
                    }
                    let chunk_to = (cursor + page).min(g_to);
                    if let Some(id) = connector_id {
                        let _ = otw_store::api_quota::bump(
                            &state.pool,
                            &crate::connectors_api::quota_scope(id),
                        )
                        .await;
                    }
                    chunks += 1;
                    match connector
                        .fetch_chunk(
                            &state.http,
                            &secrets,
                            ticker,
                            &b.asset_type,
                            &b.timeframe,
                            cursor,
                            chunk_to,
                        )
                        .await
                    {
                        Ok(chunk) => {
                            served += chunk.bars.len();
                            // Only bars inside the asked window count as an answer: some
                            // providers (Kraken) ignore the upper bound and always reply
                            // with the most recent page, which is not the past we asked for.
                            let usable: Vec<_> = chunk
                                .bars
                                .into_iter()
                                .filter(|x| x.ts >= from && x.ts < to)
                                .collect();
                            fetched += usable.len();
                            if older && !usable.is_empty() {
                                got_older = true;
                            }
                            rows.extend(usable);
                        }
                        Err(e) => {
                            // Keep whatever we already have on screen and say why the rest
                            // is missing — an unreachable provider is not an empty chart.
                            let msg = format!("{e:#}");
                            notice = Some(json!({
                                "code": notice_code(&msg),
                                "message": short_reason(&msg),
                            }));
                            break 'gaps;
                        }
                    }
                    cursor = chunk_to;
                }
            }
        }
    }

    // Providers overlap pages and ignore the window bounds; normalize to one clean series.
    rows.retain(|r| r.ts >= from && r.ts < to);
    rows.sort_by_key(|r| r.ts);
    rows.dedup_by_key(|r| r.ts);
    if rows.len() as i64 > want {
        rows.drain(..rows.len() - want as usize);
    }

    // Nothing older came back for a window we did ask about → this is the start of what
    // the provider will serve; the client stops offering "load more".
    let history_start = asked_older && notice.is_none() && !got_older;
    // It answered, but with bars from somewhere else entirely: that is a history-depth
    // refusal in disguise, and the user deserves to read it as one.
    if notice.is_none() && served > 0 && fetched == 0 {
        notice = Some(json!({
            "code": "depth",
            "message": format!(
                "{} served no bars inside this window — its history for {} may not reach that far back",
                cap.label, b.timeframe
            ),
        }));
    }

    let mut out = bars_payload(&rows);
    out["provider"] = json!(provider);
    out["connector_id"] = json!(connector_id);
    out["asset_type"] = json!(b.asset_type);
    out["ticker"] = json!(ticker);
    out["timeframe"] = json!(b.timeframe);
    out["from"] = json!(from.format(&Rfc3339).unwrap_or_default());
    out["to"] = json!(to.format(&Rfc3339).unwrap_or_default());
    out["from_store"] = json!(stored_here);
    out["from_provider"] = json!(fetched);
    out["history_start"] = json!(history_start);
    out["stream"] = json!(live::stream_capable(&provider));
    out["stored"] = dataset
        .map(|d| json!({ "id": d.id, "bar_count": d.bar_count }))
        .unwrap_or(Value::Null);
    out["notice"] = notice.unwrap_or(Value::Null);
    Ok(Json(out))
}

/// A connector whose user-declared request limit is spent, as a ready-made notice.
///
/// Quotas are observe-only everywhere else; here they do gate, because the chart can fire
/// a request per click and the user set that ceiling precisely to bound this.
async fn quota_exhausted(state: &AppState, connector_id: Option<Uuid>) -> Option<Value> {
    let id = connector_id?;
    let q = otw_store::api_quota::get(&state.pool, &crate::connectors_api::quota_scope(id))
        .await
        .ok()??;
    let max = q.max_requests?;
    (q.used >= max).then(|| {
        json!({
            "code": "quota",
            "message": format!(
                "connector request limit reached ({}/{} per {}) — resets {}",
                q.used, max, q.period,
                q.resets_at.format(&Rfc3339).unwrap_or_default()
            ),
        })
    })
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
    let connector_id = conn_store::default_for(&state.pool, &ds.provider)
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
    let mut out = bars_payload(&rows);
    out["id"] = json!(ds.id);
    out["provider"] = json!(ds.provider);
    out["asset_type"] = json!(ds.asset_type);
    out["ticker"] = json!(ds.ticker);
    out["timeframe"] = json!(ds.timeframe);
    Ok(Json(out))
}

/// Bars as parallel arrays (ts as RFC3339 strings, o/h/l/c/v as numbers) — compact and
/// ECharts-ready. Shared by the stored-bars read and the ad-hoc preview so the chart
/// consumes one shape either way.
fn bars_payload(rows: &[otw_store::histdata::Bar]) -> Value {
    let mut ts = Vec::with_capacity(rows.len());
    let (mut o, mut h, mut l, mut c, mut v) = (
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
    );
    for b in rows {
        ts.push(b.ts.format(&Rfc3339).unwrap_or_default());
        o.push(b.open);
        h.push(b.high);
        l.push(b.low);
        c.push(b.close);
        v.push(b.volume);
    }
    json!({ "count": rows.len(), "ts": ts, "o": o, "h": h, "l": l, "c": c, "v": v })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failures_are_sorted_into_codes() {
        assert_eq!(notice_code("HTTP 429 Too Many Requests"), "rate_limit");
        assert_eq!(notice_code("EODHD requires a 'api_key' credential"), "auth");
        assert_eq!(notice_code("massive (NOT_AUTHORIZED): plan does not include"), "auth");
        assert_eq!(notice_code("binance status (bad symbol?)"), "symbol");
        assert_eq!(notice_code("yahoo: 1h data is limited to the last 730 days"), "depth");
        assert_eq!(notice_code("connection reset"), "provider");
    }

    #[test]
    fn reasons_drop_the_url_tail_that_could_carry_a_key() {
        let msg = "eodhd status: HTTP status client error (401 Unauthorized) \
                   for url (https://eodhd.com/api/eod/AAPL?api_token=SECRET)";
        let short = short_reason(msg);
        assert!(!short.contains("SECRET"), "{short}");
        assert!(short.ends_with("(401 Unauthorized)"), "{short}");
    }
}
