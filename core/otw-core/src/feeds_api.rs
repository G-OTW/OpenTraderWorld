//! HTTP API for the news-feed module.
//!
//! Feeds CRUD, write-only secrets, item listing with simple filters, a manual
//! refresh, and a Server-Sent Events stream that pushes live "new items" events.
//!
//! Secrets are never returned: only their names (and "is set") are exposed.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::JsonValue;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::feeds::{self, DashboardPatch, FeedPatch, ItemFilter};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/feeds", get(list_feeds).post(create_feed))
        .route("/api/feeds/quotas", get(feed_quotas))
        .route(
            "/api/feeds/{id}",
            get(get_feed).patch(update_feed).delete(delete_feed),
        )
        .route("/api/feeds/refresh-all", post(refresh_all))
        .route("/api/feeds/{id}/refresh", post(refresh_feed))
        // Dashboards.
        .route("/api/feed-dashboards", get(list_dashboards).post(create_dashboard))
        .route(
            "/api/feed-dashboards/{id}",
            get(get_dashboard).patch(update_dashboard).delete(delete_dashboard),
        )
        .route("/api/feed-dashboards/{id}/default", post(set_default_dashboard))
        .route("/api/feed-dashboards/{id}/refresh", post(refresh_dashboard))
        .route("/api/feed-dashboards/{id}/sources", get(list_dashboard_sources).post(add_dashboard_source))
        .route("/api/feed-dashboards/{id}/sources/{feed_id}", axum::routing::delete(remove_dashboard_source))
        // Secrets: list names, set, delete. Values never leave the server.
        .route("/api/feeds/{id}/secrets", get(list_secrets).post(set_secret))
        .route("/api/feeds/{id}/secrets/{name}", axum::routing::delete(delete_secret))
        // Items with filters, plus distinct sources for filter dropdowns.
        .route("/api/feed-items", get(list_items))
        .route("/api/feed-sources", get(sources))
        // Live updates.
        .route("/api/feeds/stream", get(stream))
}

// ── Feeds CRUD ───────────────────────────────────────────────────────────────

async fn list_feeds(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let feeds = feeds::list_feeds(&s.pool).await?;
    Ok(Json(json!({ "feeds": feeds })))
}

// ── Request quotas (central api_quota, scope `feed:<id>`) ────────────────────
//
// An API source may declare `config.rate_limit = { "max": <n|null>, "period": "day" }`
// (absent = tracking off, max null = unlimited but tracked). The declaration lives in
// the feed config the form already round-trips; on every create/update we mirror it
// into the central quota table the scheduler bumps per poll.

fn feed_scope(id: Uuid) -> String {
    format!("feed:{id}")
}

/// Validate `config.rate_limit`, returning the quota to declare: None = tracking off,
/// Some((max, period)) = tracked. Called *before* persisting so a bad config never lands.
fn parse_rate_limit(config: &JsonValue) -> Result<Option<(Option<i64>, String)>, ApiError> {
    let Some(rl) = config.get("rate_limit").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let period = rl.get("period").and_then(Value::as_str).unwrap_or("day");
    if !otw_store::api_quota::valid_period(period) {
        return Err(ApiError::bad_request(
            "rate_limit.period must be minute|hour|day|week|month",
        ));
    }
    let max = rl.get("max").and_then(Value::as_i64);
    if max.is_some_and(|n| n < 1) {
        return Err(ApiError::bad_request("rate_limit.max must be at least 1"));
    }
    Ok(Some((max, period.to_string())))
}

/// Mirror a parsed rate-limit declaration into the quota table.
async fn apply_rate_limit(
    s: &AppState,
    id: Uuid,
    parsed: &Option<(Option<i64>, String)>,
) -> Result<(), ApiError> {
    match parsed {
        Some((max, period)) => otw_store::api_quota::set(&s.pool, &feed_scope(id), *max, period).await?,
        None => otw_store::api_quota::remove(&s.pool, &feed_scope(id)).await?,
    }
    Ok(())
}

/// GET /api/feeds/quotas — current usage of every tracked feed, keyed by feed id.
async fn feed_quotas(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let quotas = otw_store::api_quota::list_prefixed(&s.pool, "feed:").await?;
    let map: serde_json::Map<String, Value> = quotas
        .into_iter()
        .map(|q| {
            let id = q.scope.trim_start_matches("feed:").to_string();
            (id, serde_json::to_value(&q).unwrap_or(Value::Null))
        })
        .collect();
    Ok(Json(json!({ "quotas": map })))
}

async fn get_feed(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let feed = feeds::get_feed(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("feed not found"))?;
    let secret_names = feeds::list_secret_names(&s.pool, id).await?;
    Ok(Json(json!({ "feed": feed, "secret_names": secret_names })))
}

#[derive(Deserialize)]
struct CreateFeed {
    #[serde(default)]
    name: String,
    kind: String,
    #[serde(default = "empty_obj")]
    config: JsonValue,
    #[serde(default = "default_interval")]
    interval_secs: i32,
}
fn empty_obj() -> JsonValue {
    json!({})
}
fn default_interval() -> i32 {
    900
}

async fn create_feed(
    State(s): State<AppState>,
    Json(body): Json<CreateFeed>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(body.kind.as_str(), "rss" | "api") {
        return Err(ApiError::bad_request("kind must be 'rss' or 'api'"));
    }
    let interval = body.interval_secs.clamp(30, 86_400);
    let rate_limit = parse_rate_limit(&body.config)?;
    let feed = feeds::create_feed(&s.pool, &body.name, &body.kind, &body.config, interval).await?;
    feeds::recompute_dedup_hash(&s.pool, feed.id).await?;
    apply_rate_limit(&s, feed.id, &rate_limit).await?;
    Ok(Json(json!({ "feed": feed })))
}

async fn update_feed(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut patch): Json<FeedPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(i) = patch.interval_secs {
        patch.interval_secs = Some(i.clamp(30, 86_400));
    }
    let touches_identity = patch.config.is_some();
    let rate_limit = patch.config.as_ref().map(parse_rate_limit).transpose()?;
    let ok = feeds::update_feed(&s.pool, id, &patch).await?;
    if !ok {
        return Err(ApiError::not_found("feed not found"));
    }
    if touches_identity {
        feeds::recompute_dedup_hash(&s.pool, id).await?;
    }
    if let Some(rl) = rate_limit {
        apply_rate_limit(&s, id, &rl).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_feed(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::delete_feed(&s.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("feed not found"));
    }
    otw_store::api_quota::remove(&s.pool, &feed_scope(id)).await?;
    Ok(Json(json!({ "ok": true })))
}

/// POST /api/feeds/{id}/refresh — poll the feed immediately.
async fn refresh_feed(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let n = s
        .scheduler
        .poll_feed_id(id)
        .await
        .map_err(|e| ApiError::bad_request(&format!("refresh failed: {e}")))?;
    Ok(Json(json!({ "new_items": n })))
}

/// POST /api/feeds/refresh-all — poll every enabled feed once.
async fn refresh_all(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let n = s
        .scheduler
        .poll_all_enabled()
        .await
        .map_err(|e| ApiError::bad_request(&format!("refresh failed: {e}")))?;
    Ok(Json(json!({ "new_items": n })))
}

// ── Secrets (write-only) ─────────────────────────────────────────────────────

async fn list_secrets(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let names = feeds::list_secret_names(&s.pool, id).await?;
    Ok(Json(json!({ "secret_names": names })))
}

#[derive(Deserialize)]
struct SetSecret {
    name: String,
    value: String,
}

async fn set_secret(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetSecret>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("secret name is required"));
    }
    if feeds::get_feed(&s.pool, id).await?.is_none() {
        return Err(ApiError::not_found("feed not found"));
    }
    feeds::set_secret(&s.pool, &s.cipher, id, body.name.trim(), &body.value).await?;
    feeds::recompute_dedup_hash(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_secret(
    State(s): State<AppState>,
    Path((id, name)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::delete_secret(&s.pool, id, &name).await?;
    if !ok {
        return Err(ApiError::not_found("secret not found"));
    }
    feeds::recompute_dedup_hash(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── Items + filters ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ItemQuery {
    q: Option<String>,
    // Comma-separated list of source names; match any (empty = all).
    source_names: Option<String>,
    source_type: Option<String>,
    feed_id: Option<Uuid>,
    dashboard_id: Option<Uuid>,
    since: Option<String>, // RFC3339
    until: Option<String>,
    // Keyset cursor for infinite scroll (echo back an item's sort_key + id).
    before_key: Option<String>, // RFC3339
    before_id: Option<Uuid>,
    limit: Option<i64>,
}

async fn list_items(
    State(s): State<AppState>,
    Query(q): Query<ItemQuery>,
) -> Result<Json<Value>, ApiError> {
    let parse = |s: &Option<String>| -> Result<Option<OffsetDateTime>, ApiError> {
        match s {
            Some(v) if !v.is_empty() => OffsetDateTime::parse(v, &Rfc3339)
                .map(Some)
                .map_err(|_| ApiError::bad_request("since/until must be RFC3339")),
            _ => Ok(None),
        }
    };
    let filter = ItemFilter {
        q: q.q.filter(|s| !s.is_empty()),
        source_names: q
            .source_names
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        source_type: q.source_type.filter(|s| !s.is_empty()),
        feed_id: q.feed_id,
        dashboard_id: q.dashboard_id,
        since: parse(&q.since)?,
        until: parse(&q.until)?,
        before_key: parse(&q.before_key)?,
        before_id: q.before_id,
        limit: q.limit.unwrap_or(100),
    };
    let items = feeds::list_items(&s.pool, &filter).await?;
    Ok(Json(json!({ "items": items })))
}

async fn sources(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let (names, types) = feeds::distinct_sources(&s.pool).await?;
    Ok(Json(json!({ "source_names": names, "source_types": types })))
}

// ── Dashboards ───────────────────────────────────────────────────────────────

async fn list_dashboards(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let dashboards = feeds::list_dashboards(&s.pool).await?;
    Ok(Json(json!({ "dashboards": dashboards })))
}

async fn get_dashboard(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let dash = feeds::get_dashboard(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dashboard not found"))?;
    Ok(Json(json!({ "dashboard": dash })))
}

#[derive(Deserialize)]
struct CreateDashboard {
    #[serde(default)]
    name: String,
}

async fn create_dashboard(
    State(s): State<AppState>,
    Json(body): Json<CreateDashboard>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("dashboard name is required"));
    }
    let dash = feeds::create_dashboard(&s.pool, name).await?;
    Ok(Json(json!({ "dashboard": dash })))
}

async fn update_dashboard(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<DashboardPatch>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::update_dashboard(&s.pool, id, &patch).await?;
    if !ok {
        return Err(ApiError::not_found("dashboard not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn set_default_dashboard(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::set_default_dashboard(&s.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("dashboard not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_dashboard(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::delete_dashboard(&s.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("dashboard not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn refresh_dashboard(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let n = s
        .scheduler
        .poll_dashboard(id)
        .await
        .map_err(|e| ApiError::bad_request(&format!("refresh failed: {e}")))?;
    Ok(Json(json!({ "new_items": n })))
}

async fn list_dashboard_sources(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let sources = feeds::dashboard_sources(&s.pool, id).await?;
    Ok(Json(json!({ "feeds": sources })))
}

/// Add a source to a dashboard. Two modes:
///   - `feed_id` set → link an existing (possibly shared) source.
///   - otherwise create a new source from `kind`/`config`/`interval_secs`,
///     unless `force` is false and an identical source already exists, in which
///     case we return `{ duplicate: <match> }` so the client can prompt to reuse.
#[derive(Deserialize)]
struct AddSource {
    feed_id: Option<Uuid>,
    #[serde(default)]
    name: String,
    kind: Option<String>,
    #[serde(default = "empty_obj")]
    config: JsonValue,
    #[serde(default = "default_interval")]
    interval_secs: i32,
    /// Suppress the duplicate check and create a fresh source anyway.
    #[serde(default)]
    force: bool,
}

async fn add_dashboard_source(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddSource>,
) -> Result<Json<Value>, ApiError> {
    if feeds::get_dashboard(&s.pool, id).await?.is_none() {
        return Err(ApiError::not_found("dashboard not found"));
    }
    let interval = body.interval_secs.clamp(30, 86_400);

    // Mode 1: link an existing source by id.
    if let Some(feed_id) = body.feed_id {
        if feeds::get_feed(&s.pool, feed_id).await?.is_none() {
            return Err(ApiError::not_found("source not found"));
        }
        feeds::link_source(&s.pool, id, feed_id, interval).await?;
        return Ok(Json(json!({ "feed_id": feed_id, "linked": true })));
    }

    // Mode 2: create a new source. Validate, then dedup-check unless forced.
    let kind = body.kind.as_deref().unwrap_or("");
    if !matches!(kind, "rss" | "api") {
        return Err(ApiError::bad_request("kind must be 'rss' or 'api'"));
    }
    if !body.force {
        // New sources carry no secrets yet, so the hash uses empty secret names.
        let hash = feeds::dedup_hash(kind, &body.config, &[]);
        if let Some(m) = feeds::find_by_hash(&s.pool, &hash).await? {
            return Ok(Json(json!({ "duplicate": m })));
        }
    }
    let rate_limit = parse_rate_limit(&body.config)?;
    let feed = feeds::create_feed(&s.pool, &body.name, kind, &body.config, interval).await?;
    feeds::recompute_dedup_hash(&s.pool, feed.id).await?;
    apply_rate_limit(&s, feed.id, &rate_limit).await?;
    feeds::link_source(&s.pool, id, feed.id, interval).await?;
    Ok(Json(json!({ "feed": feed, "linked": true })))
}

async fn remove_dashboard_source(
    State(s): State<AppState>,
    Path((id, feed_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let ok = feeds::unlink_source(&s.pool, id, feed_id).await?;
    if !ok {
        return Err(ApiError::not_found("source not linked to this dashboard"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Live updates (SSE) ───────────────────────────────────────────────────────

/// GET /api/feeds/stream — Server-Sent Events; emits a JSON event per poll that
/// produced new items. The client refetches items on each event.
async fn stream(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = s.scheduler.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(ev) => Some(Ok(Event::default()
            .event("feed")
            .data(serde_json::to_string(&ev).unwrap_or_default()))),
        // Dropped messages (lagging receiver) are skipped.
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
