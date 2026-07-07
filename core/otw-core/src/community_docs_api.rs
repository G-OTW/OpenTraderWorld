//! HTTP API for the Community Docs module.
//!
//! Docs are authored on the website and synced into the app so they stay available
//! offline. The list omits the body; the body is fetched per-slug. Refresh pulls the
//! website's published feed and upserts it by slug; the sync endpoint accepts the same
//! batch shape pushed by a caller and remains exercisable manually.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};
use otw_store::community_docs::{self, DocInput};

/// Where submitted docs are relayed for editorial review. Fixed at build time; the review
/// site verifies the bearer token (env `DOC_SUBMISSION_TOKEN`) so this endpoint can't be
/// spammed directly. The token is read server-side and never reaches the frontend bundle.
const SUBMISSION_URL: &str = "https://opentraderworld.com/api/doc-submissions";

/// Where the published docs library is pulled from. The website is the source of truth:
/// its feed returns every published doc keyed by immutable slug, matching `DocInput`
/// field-for-field. Fixed at build time like `SUBMISSION_URL`.
const FEED_URL: &str = "https://opentraderworld.com/api/docs/feed";

/// Rolling rate-limit window for the relay. Guards the review site against an abused
/// instance flooding it. Single-user self-hosted, so a small allowance is plenty.
const SUBMIT_MAX_PER_WINDOW: usize = 20;
const SUBMIT_WINDOW: Duration = Duration::from_secs(3600);

/// In-memory sliding-window limiter for the submission relay. Cloneable; shares state.
#[derive(Clone, Default)]
pub struct SubmitLimiter {
    hits: Arc<Mutex<Vec<Instant>>>,
}

impl SubmitLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an attempt; returns `false` if the window is already full.
    fn allow(&self) -> bool {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap();
        hits.retain(|t| now.duration_since(*t) < SUBMIT_WINDOW);
        if hits.len() >= SUBMIT_MAX_PER_WINDOW {
            return false;
        }
        hits.push(now);
        true
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/community-docs", get(list_docs))
        .route("/api/community-docs/favorites", get(list_favorites))
        .route("/api/community-docs/refresh", post(refresh_docs))
        .route("/api/community-docs/sync", post(sync_docs))
        .route("/api/community-docs/submit", post(submit_doc))
        .route("/api/community-docs/{slug}", get(get_doc))
        .route("/api/community-docs/{slug}/favorite", put(set_favorite))
}

async fn list_docs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let docs = community_docs::list_docs(&state.pool).await?;
    Ok(Json(json!({ "docs": docs })))
}

async fn list_favorites(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let docs = community_docs::list_favorites(&state.pool).await?;
    Ok(Json(json!({ "docs": docs })))
}

#[derive(Debug, Deserialize)]
struct FavoriteBody {
    favorite: bool,
}

/// Pin/unpin a doc. Favorites persist across syncs and refreshes.
async fn set_favorite(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<FavoriteBody>,
) -> Result<Json<Value>, ApiError> {
    let ok = community_docs::set_favorite(&state.pool, &slug, body.favorite).await?;
    if !ok {
        return Err(ApiError::not_found("doc not found"));
    }
    Ok(Json(json!({ "favorited": body.favorite })))
}

/// The website feed's payload: `{ "docs": [DocInput, ...] }`. Field names are the
/// contract shared with the website's `/api/docs/feed`.
#[derive(Debug, Deserialize, Default)]
struct FeedBody {
    #[serde(default)]
    docs: Vec<DocInput>,
}

/// Reload docs from the website feed without touching favorites.
///
/// Pulls every published doc and upserts by slug, so it is safe to call repeatedly:
/// new docs appear, edited docs update, favorites are never removed here. Malformed
/// entries (empty slug) are skipped rather than failing the whole sync.
async fn refresh_docs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let res = state.http.get(FEED_URL).send().await.map_err(|e| {
        tracing::error!("docs feed fetch failed: {e}");
        ApiError::bad_gateway("could not reach the docs feed")
    })?;

    if !res.status().is_success() {
        let code = res.status().as_u16();
        tracing::warn!("docs feed returned HTTP {code}");
        return Err(ApiError::bad_gateway(&format!(
            "docs feed returned HTTP {code}"
        )));
    }

    let feed: FeedBody = res.json().await.map_err(|e| {
        tracing::error!("docs feed returned invalid JSON: {e}");
        ApiError::bad_gateway("docs feed returned an invalid payload")
    })?;

    let mut refreshed = 0;
    for mut input in feed.docs {
        input.slug = input.slug.trim().to_string();
        if input.slug.is_empty() {
            continue;
        }
        community_docs::upsert_doc(&state.pool, &input).await?;
        refreshed += 1;
    }

    let docs = community_docs::list_docs(&state.pool).await?;
    Ok(Json(json!({ "refreshed": refreshed, "docs": docs })))
}

async fn get_doc(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match community_docs::get_doc(&state.pool, &slug).await? {
        Some(doc) => Ok(Json(json!({ "doc": doc }))),
        None => Err(ApiError::not_found("doc not found")),
    }
}

#[derive(Debug, Deserialize, Default)]
struct SyncBody {
    #[serde(default)]
    docs: Vec<DocInput>,
}

/// Upsert a batch of docs by slug. Returns how many were synced.
async fn sync_docs(
    State(state): State<AppState>,
    Json(body): Json<SyncBody>,
) -> Result<Json<Value>, ApiError> {
    let mut synced = 0;
    for mut input in body.docs {
        input.slug = input.slug.trim().to_string();
        if input.slug.is_empty() {
            return Err(ApiError::bad_request("doc slug required"));
        }
        community_docs::upsert_doc(&state.pool, &input).await?;
        synced += 1;
    }
    Ok(Json(json!({ "synced": synced })))
}

/// A doc submitted from the editor for editorial review. Rendered HTML (`html`) is the
/// faithful body; `source_json` is the ProseMirror doc kept so the submission can be
/// re-edited later. `author` fields are optional and only sent if the user opts in.
#[derive(Debug, Deserialize)]
struct SubmitBody {
    #[serde(default)]
    title: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    html: String,
    #[serde(default)]
    source_json: Value,
    #[serde(default)]
    language: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    author_name: Option<String>,
    #[serde(default)]
    author_email: Option<String>,
}

/// Relay a doc submission to the review website. The bearer token lives in the server env
/// (`DOC_SUBMISSION_TOKEN`) and is added here, so it never ships in the frontend. Rate-
/// limited in-memory to protect the review site from an abused instance.
async fn submit_doc(
    State(state): State<AppState>,
    Json(body): Json<SubmitBody>,
) -> Result<Json<Value>, ApiError> {
    if body.title.trim().is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    if body.html.trim().is_empty() {
        return Err(ApiError::bad_request("empty document"));
    }
    if !state.submit_limiter.allow() {
        return Err(ApiError::too_many(
            "submission rate limit reached — try again later",
        ));
    }

    let token = std::env::var("DOC_SUBMISSION_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| ApiError::internal("submissions not configured on this server"))?;

    // Stamp the submission time server-side; don't trust a client-provided clock.
    let submitted_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let payload = json!({
        "title": body.title,
        "icon": body.icon,
        "layout": body.layout,
        "html": body.html,
        "source_json": body.source_json,
        "language": body.language,
        "categories": body.categories,
        "submitted_at": submitted_at,
        "author": { "name": body.author_name, "email": body.author_email },
    });

    let res = state
        .http
        .post(SUBMISSION_URL)
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("doc submission relay failed: {e}");
            ApiError::bad_gateway("could not reach the review service")
        })?;

    if !res.status().is_success() {
        let code = res.status().as_u16();
        tracing::warn!("review service rejected submission: HTTP {code}");
        return Err(ApiError::bad_gateway(&format!(
            "review service returned HTTP {code}"
        )));
    }

    Ok(Json(json!({ "submitted": true })))
}
