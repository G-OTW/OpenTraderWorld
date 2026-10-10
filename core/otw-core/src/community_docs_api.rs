//! HTTP API for the Community Docs module.
//!
//! Docs are authored on the website and synced into the app so they stay available
//! offline. The list omits the body; the body is fetched per-slug. Refresh pulls the
//! website's published feed and upserts it by slug; the sync endpoint accepts the same
//! batch shape pushed by a caller and remains exercisable manually.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use uuid::Uuid;

use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};
use otw_store::community_docs::{self, DocInput};

/// Where submitted docs are relayed for editorial review. Fixed at build time. Anonymous
/// submissions are accepted (the review site rate-limits per IP and every item is human-
/// reviewed); an optional bearer token (env `DOC_SUBMISSION_TOKEN`) marks this instance as
/// trusted. The token is read server-side and never reaches the frontend bundle.
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

// ── Body sanitisation ────────────────────────────────────────────────────────

/// Inline SVG tags a doc body may keep. Mirrors the website's own allowlist
/// (`OTWWebsite/lib/sanitize.js`), which is what the published site already renders:
/// diagrams are drawn as inline SVG, so stripping them here left every synced doc with a
/// hole where its figure used to be. `script`, `style`, `foreignObject`, `use` and the
/// animation elements are deliberately absent: those are the ways an SVG runs code or
/// reaches back out to the network.
const SVG_TAGS: &[&str] = &[
    "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text",
    "tspan", "defs", "marker", "title", "desc",
];

/// Attributes those tags may carry: geometry and presentation only, no event handlers
/// (`on*` is not on the list, so ammonia drops every one) and no URL attribute.
const SVG_ATTRS: &[&str] = &[
    "viewBox", "viewbox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r",
    "rx", "ry", "d", "points", "fill", "stroke", "stroke-width", "stroke-dasharray",
    "stroke-linecap", "stroke-linejoin", "transform", "text-anchor", "font-size", "font-weight",
    "opacity", "fill-opacity", "stroke-opacity", "role", "aria-label", "xmlns",
];

/// Ammonia configuration for a doc body.
///
/// Doc bodies are rendered with `{@html}` on the docs page, so whatever is stored here
/// executes on the app's own origin. They arrive from the website feed over the network,
/// which makes that site a single point able to run script on every self-hosted instance
/// that clicks Refresh — the one host the operator does not control. "Curated upstream" is
/// an assumption, not a control; clean at ingest so the stored body is safe by
/// construction, whatever the source turns out to be.
///
/// The rule for what it accepts is **parity with the publisher**: every doc body comes from
/// the website, whose own allowlist (`OTWWebsite/lib/sanitize.js`) is sized to the markup
/// this app's editor produces. Anything the site publishes but this list omits does not
/// arrive degraded, it arrives missing, which is how images, inline SVG, highlights and
/// task lists were silently disappearing on sync. Widen the two lists together.
fn body_cleaner() -> &'static ammonia::Builder<'static> {
    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    CLEANER.get_or_init(|| {
        let mut b = ammonia::Builder::empty();
        b.tags(
            [
                "p", "br", "hr", "strong", "b", "em", "i", "u", "s", "sub", "sup", "code", "pre",
                "kbd", "samp", "blockquote", "ul", "ol", "li", "dl", "dt", "dd", "h1", "h2", "h3",
                "h4", "h5", "h6", "a", "span", "div", "img", "figure", "figcaption", "table",
                "thead", "tbody", "tfoot", "tr", "th", "td", "caption", "details", "summary",
                // Highlight marks, and the inert checkbox of a task list.
                "mark", "label", "input",
            ]
            .into_iter()
            .chain(SVG_TAGS.iter().copied())
            .collect(),
        )
        .add_tag_attributes("a", ["href", "title"])
        .add_tag_attributes("img", ["src", "alt", "title", "width", "height", "style"])
        .add_tag_attributes("td", ["colspan", "rowspan"])
        .add_tag_attributes("th", ["colspan", "rowspan", "scope"])
        .add_tag_attributes("ol", ["start"])
        .add_tag_attributes("code", ["class"])
        .add_tag_attributes("pre", ["class"])
        .add_tag_attributes("span", ["class", "style"])
        .add_tag_attributes("div", ["class"])
        .add_tag_attributes("mark", ["style", "data-color"])
        // Task lists: TipTap marks the list and its items, the checkbox only shows state.
        .add_tag_attributes("ul", ["data-type"])
        .add_tag_attributes("li", ["data-type", "data-checked"])
        .add_tag_attributes("input", ["checked"])
        // A checkbox is state, never an input a reader can operate: `type` and `disabled`
        // are forced rather than accepted, so no other kind of control can be smuggled in.
        .set_tag_attribute_value("input", "type", "checkbox")
        .set_tag_attribute_value("input", "disabled", "disabled")
        // The video-embed facade is a figure carrying one class, and only that class.
        // `add_allowed_classes` is what grants `class` here: listing it in the tag's
        // attributes as well would make it unrestricted, and ammonia asserts against it.
        .add_allowed_classes("figure", [VIDEO_FIGURE_CLASS])
        // Inline styles are the editor's image width and its colour marks, nothing else.
        // Property names are narrowed here; the values are checked in `attribute_filter`.
        .filter_style_properties(
            ["width", "color", "font-size", "background-color"]
                .into_iter()
                .collect(),
        )
        .link_rel(Some("noopener noreferrer nofollow"))
        .set_tag_attribute_value("a", "target", "_blank")
        // `data:` is admitted at the scheme gate so a self-contained image can survive the
        // trip from the website, then narrowed to exactly that by `attribute_filter` below:
        // ammonia checks schemes first and runs the filter after, so this pair is the only
        // way to say "data: on <img src>, and only for a raster MIME type". Everything else
        // carrying a data: URL (an `<a href>`, an SVG-typed image) is dropped there.
        .url_schemes(["http", "https", "mailto", "data"].into_iter().collect())
        .attribute_filter(|tag, attr, value| {
            if attr == "style" {
                // No CSS function survives, at any property. `url()` and `image-set()` are
                // the only way an inline style reaches the network, and refusing the whole
                // function syntax is a rule with no corners to get wrong.
                return (!value.contains('(')).then(|| value.into());
            }
            if !is_data_url(value) {
                return Some(value.into());
            }
            (tag == "img" && attr == "src" && is_safe_image_data_url(value)).then(|| value.into())
        });
        for tag in SVG_TAGS {
            b.add_tag_attributes(*tag, SVG_ATTRS.iter().copied());
        }
        b
    })
}

/// Class that marks the video-embed facade, shared by the app and the website renderer.
const VIDEO_FIGURE_CLASS: &str = "otw-video";

/// Largest `data:` image a doc body may carry. The submission relay refuses a payload past
/// 7.5 MB in total, so a single image beyond this is malformed rather than merely big.
const MAX_DATA_URL_BYTES: usize = 8 * 1024 * 1024;

/// Normalise a URL attribute the way the WHATWG parser does before it is inspected: ASCII
/// tab/CR/LF are removed anywhere, leading and trailing C0 controls and spaces are trimmed.
/// Without this, `da&#9;ta:` would parse as `data:` for the browser but not for us.
fn normalize_url_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect::<String>()
        .trim_matches(|c: char| c <= ' ')
        .to_string()
}

fn is_data_url(value: &str) -> bool {
    let v = normalize_url_value(value);
    v.len() >= 5 && v[..5].eq_ignore_ascii_case("data:")
}

/// A `data:` URL is only kept when it is a base64 raster image.
///
/// The MIME type is what decides how a browser treats the bytes, so the allowlist is the
/// control: none of these types can execute, and `image/svg+xml` (which can) is absent.
/// The payload alphabet is checked too, so a URL cannot smuggle a second `,` and re-declare
/// its own type.
fn is_safe_image_data_url(value: &str) -> bool {
    const TYPES: &[&str] = &[
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/jpg;base64,",
        "data:image/gif;base64,",
        "data:image/webp;base64,",
        "data:image/avif;base64,",
    ];
    let v = normalize_url_value(value);
    if v.len() > MAX_DATA_URL_BYTES {
        return false;
    }
    let Some(payload) = TYPES.iter().find_map(|p| {
        (v.len() >= p.len() && v[..p.len()].eq_ignore_ascii_case(p)).then(|| &v[p.len()..])
    }) else {
        return false;
    };
    !payload.is_empty()
        && payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b' '))
}

/// Clean one incoming doc in place. Applied on every write path (feed refresh and the
/// sync endpoint alike) so no unsanitised body can reach the database by any route.
fn clean_doc(input: &mut DocInput) {
    input.body = body_cleaner().clean(&input.body).to_string();
    // Title and summary render as text, never as markup — strip tags outright.
    input.title = ammonia::Builder::empty().clean(&input.title).to_string();
    input.summary = ammonia::Builder::empty().clean(&input.summary).to_string();
}

async fn list_docs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let docs = community_docs::list_docs(&state.pool).await?;
    Ok(Json(json!({ "docs": docs })))
}

async fn list_favorites(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let docs = community_docs::list_favorites(&state.pool).await?;
    Ok(Json(json!({ "docs": docs })))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FavoriteBody {
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
        clean_doc(&mut input);
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
        clean_doc(&mut input);
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

/// Prefix of internal upload URLs the review site cannot reach; inlined before relaying.
const FILE_SRC_PREFIX: &str = "/api/files/";

/// The review site caps a submission at 8 MB; refuse before sending a doomed request,
/// with headroom for JSON escaping and the non-image fields.
const MAX_SUBMIT_PAYLOAD_BYTES: usize = 7_500_000;

/// Uuid of an uploaded file if `src` is exactly an internal upload URL (`/api/files/{id}`).
fn upload_file_id(src: &str) -> Option<Uuid> {
    Uuid::parse_str(src.strip_prefix(FILE_SRC_PREFIX)?).ok()
}

/// Every `src="..."` attribute value in `html`, in document order.
fn collect_html_srcs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("src=\"") {
        rest = &rest[pos + 5..];
        let Some(end) = rest.find('"') else { break };
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// Visit the source of every media node in a ProseMirror doc: an `image`'s `src` and a
/// `videoEmbed`'s `poster`. A video embed stores no bytes of its own, but its thumbnail is
/// an ordinary upload and has to make the same trip as any other picture.
fn visit_media_srcs(node: &mut Value, f: &mut impl FnMut(&mut String)) {
    match node {
        Value::Object(map) => {
            let attr = match map.get("type").and_then(Value::as_str) {
                Some("image") => Some("src"),
                Some("videoEmbed") => Some("poster"),
                _ => None,
            };
            if let Some(attr) = attr {
                if let Some(Value::Object(attrs)) = map.get_mut("attrs") {
                    if let Some(Value::String(src)) = attrs.get_mut(attr) {
                        f(src);
                    }
                }
            }
            for v in map.values_mut() {
                visit_media_srcs(v, f);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                visit_media_srcs(v, f);
            }
        }
        _ => {}
    }
}

/// Replace internal upload image URLs in `html` and `source_json` with data: URIs so the
/// review site gets self-contained images. Non-image or missing files are left as-is.
/// Only the relayed copy is rewritten; nothing stored in the app changes.
async fn inline_upload_images(
    state: &AppState,
    html: &mut String,
    source_json: &mut Value,
) -> Result<(), ApiError> {
    let mut srcs: BTreeSet<String> = collect_html_srcs(html)
        .into_iter()
        .filter(|s| upload_file_id(s).is_some())
        .collect();
    visit_media_srcs(source_json, &mut |src| {
        if upload_file_id(src).is_some() {
            srcs.insert(src.clone());
        }
    });

    let mut inlined: HashMap<String, String> = HashMap::new();
    for src in srcs {
        let Some(id) = upload_file_id(&src) else { continue };
        let Some(meta) = otw_store::files::get(&state.pool, id).await? else {
            continue;
        };
        // Only image/* with an attribute-safe content type; anything else stays a link.
        let ct = meta.content_type;
        if !ct.starts_with("image/")
            || !ct
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'+' | b'-'))
        {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(state.upload_dir.join(id.to_string())).await else {
            continue;
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        inlined.insert(src, format!("data:{ct};base64,{b64}"));
    }

    for (src, data) in &inlined {
        *html = html.replace(&format!("src=\"{src}\""), &format!("src=\"{data}\""));
    }
    visit_media_srcs(source_json, &mut |src| {
        if let Some(data) = inlined.get(src.as_str()) {
            *src = data.clone();
        }
    });
    Ok(())
}

/// First media source in the submission that is neither an inlined image nor a resolvable
/// upload, in both the HTML and the ProseMirror copy. `None` means the doc is self-contained
/// and will render with nothing to fetch.
///
/// Run *after* `inline_upload_images`: an `/api/files/{id}` left over at that point is a
/// missing or non-image upload, which is just as unpublishable as a remote URL.
fn first_non_self_contained_src(html: &str, source_json: &Value) -> Option<String> {
    let mut offender = None;
    let mut check = |src: &str| {
        if offender.is_none() && !src.is_empty() && !is_safe_image_data_url(src) {
            offender = Some(src.chars().take(120).collect::<String>());
        }
    };
    for src in collect_html_srcs(html) {
        check(&src);
    }
    let mut json = source_json.clone();
    visit_media_srcs(&mut json, &mut |src| check(src));
    offender
}

/// Relay a doc submission to the review website. An optional bearer token from the server
/// env (`DOC_SUBMISSION_TOKEN`) is added here, so it never ships in the frontend. Rate-
/// limited in-memory to protect the review site from an abused instance.
async fn submit_doc(
    State(state): State<AppState>,
    Json(mut body): Json<SubmitBody>,
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

    // Optional: the review site accepts anonymous submissions (rate-limited, human-reviewed).
    // When set, the token marks this instance as trusted.
    let token = std::env::var("DOC_SUBMISSION_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty());

    // The review site can't reach this instance's /api/files/{id} URLs — inline them.
    inline_upload_images(&state, &mut body.html, &mut body.source_json).await?;

    // Everything that survives has to be self-contained, because a published doc travels
    // back down the feed into every instance and is read offline. A remote `<img>` renders
    // nowhere useful (the website's CSP is `img-src 'self' data:`) and would beacon the
    // reader's IP to whoever hosts it, so the submission is refused with something the
    // author can act on rather than published with a hole in it.
    if let Some(src) = first_non_self_contained_src(&body.html, &body.source_json) {
        tracing::info!("doc submission refused: image not self-contained ({src})");
        return Err(ApiError::bad_request(
            "an image in this document is linked from another site. Upload the picture into \
             the document (drag it in, or /image upload) so the published doc works offline.",
        ));
    }

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

    let payload_bytes = serde_json::to_vec(&payload).map(|v| v.len()).unwrap_or(usize::MAX);
    if payload_bytes > MAX_SUBMIT_PAYLOAD_BYTES {
        return Err(ApiError::bad_request(
            "document too large to submit — remove or shrink some images",
        ));
    }

    let mut req = state.http.post(SUBMISSION_URL).json(&payload);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let res = req
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

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        body_cleaner().clean(html).to_string()
    }

    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";

    /// Exactly what `opentraderworld.com` stores in `docs.body` and serves from
    /// `/api/docs/feed` for a document holding every media kind the editor can produce.
    /// Regenerate with `node scripts/docs-media/check.mjs` in the website repo, which
    /// asserts the same shape from the other side.
    const PUBLISHED_BODY: &str = r##"<p>Body</p><p><mark data-color="#ffd54f" style="background-color:#ffd54f">lit</mark><span style="color:#22c55e;font-size:18px">green</span></p><ul data-type="taskList"><li data-type="taskItem" data-checked="true"><label><input type="checkbox" disabled="disabled" checked="checked" /></label><div><p>done</p></div></li></ul><img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=" alt="A chart" style="width:320px" /><figure class="otw-video"><a href="https://www.youtube.com/watch?v=dQw4w9WgXcQ" target="_blank" rel="noopener noreferrer nofollow"><img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=" alt="How sizing works" /></a><figcaption>How sizing works</figcaption></figure>"##;

    #[test]
    fn a_published_doc_arrives_whole() {
        // The end of the round trip: app editor to the review site to `docs.body` to the
        // feed to here. Every assertion below is something that used to be dropped.
        let out = clean(PUBLISHED_BODY);

        assert!(out.contains("data:image/png;base64,iVBORw0KGgo"), "image lost: {out}");
        assert_eq!(out.matches("data:image/png;base64,").count(), 2, "{out}");
        assert!(out.contains("style=\"width:320px\""), "image width lost: {out}");
        assert!(out.contains("alt=\"A chart\""), "{out}");

        assert!(out.contains("<figure class=\"otw-video\">"), "video facade lost: {out}");
        assert!(
            out.contains("href=\"https://www.youtube.com/watch?v=dQw4w9WgXcQ\""),
            "video link lost: {out}"
        );
        assert!(out.contains("<figcaption>How sizing works</figcaption>"), "{out}");

        assert!(out.contains("background-color:#ffd54f"), "highlight lost: {out}");
        assert!(out.contains("color:#22c55e"), "text colour lost: {out}");
        assert!(out.contains("font-size:18px"), "font size lost: {out}");

        assert!(out.contains("data-type=\"taskList\""), "task list lost: {out}");
        assert!(out.contains("data-checked=\"true\""), "{out}");
        assert!(out.contains("type=\"checkbox\""), "{out}");
        assert!(out.contains("disabled"), "checkbox must stay inert: {out}");

        // Nothing executable came along for the ride.
        assert!(!out.contains("script"), "{out}");
        assert!(!out.to_ascii_lowercase().contains("javascript:"), "{out}");
    }

    #[test]
    fn an_inline_style_can_never_fetch_or_overlay() {
        // Property names are narrowed by ammonia; the function syntax, which is the only
        // way a style reaches the network, is refused whole.
        let out = clean(
            r#"<img src="x" style="width: 10px; position: fixed; background: url(https://evil.test/beacon)">"#,
        );
        assert!(!out.contains("url("), "{out}");
        assert!(!out.contains("position"), "{out}");
        let out = clean(r#"<span style="color: red">a</span>"#);
        assert!(out.contains("color:red"), "{out}");
    }

    #[test]
    fn a_checkbox_can_never_become_another_control() {
        let out = clean(r#"<input type="text" name="pw" value="x" autofocus>"#);
        assert!(out.contains(r#"type="checkbox""#), "{out}");
        assert!(out.contains("disabled"), "{out}");
        assert!(!out.contains("name="), "{out}");
        assert!(!out.contains("value="), "{out}");
        assert!(!out.contains("autofocus"), "{out}");
    }

    #[test]
    fn a_self_contained_image_survives_the_sync() {
        // The submission relay inlines every upload as a data: URI, so this is the shape
        // every image in a published doc actually has. Dropping it left docs image-less.
        let out = clean(&format!(r#"<p><img src="{PNG}" alt="chart"></p>"#));
        assert!(out.contains(PNG), "data: image was stripped: {out}");
        assert!(out.contains(r#"alt="chart""#));
    }

    #[test]
    fn a_data_url_is_refused_everywhere_it_could_execute() {
        // Only <img src> takes a data: URL, and only for a raster MIME type.
        for html in [
            r#"<a href="data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==">x</a>"#,
            r#"<img src="data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Lz48L3N2Zz4=">"#,
            r#"<img src="data:text/html,<script>alert(1)</script>">"#,
            r#"<img src="data:image/png,<script>alert(1)</script>">"#,
            // A second declaration smuggled into the payload.
            r#"<img src="data:image/png;base64,x,text/html;base64,PHN2Zz4=">"#,
        ] {
            let out = clean(html);
            assert!(!out.contains("data:"), "data: URL survived: {out}");
        }
    }

    #[test]
    fn a_data_url_cannot_hide_behind_whitespace_normalisation() {
        // The URL parser removes tab/CR/LF anywhere and trims leading control characters,
        // so the check has to normalise the same way before it decides.
        let out = clean("<a href=\"da\tta:text/html,<b>x</b>\">y</a>");
        assert!(!out.contains("data:"), "obfuscated data: URL survived: {out}");
        let out = clean(&format!("<img src=\" \r\n{PNG}\">"));
        assert!(out.contains("base64,iVBORw0KGgoAAAANSUhEUg=="), "{out}");
    }

    #[test]
    fn inline_svg_survives_but_cannot_carry_code() {
        // The website publishes diagrams as inline SVG; stripping the tags here left a
        // hole in every synced doc.
        let out = clean(
            r#"<svg viewBox="0 0 10 10" role="img" aria-label="d"><rect x="1" y="1" width="8" height="8" fill="green"/></svg>"#,
        );
        assert!(out.contains("<svg"), "svg was stripped: {out}");
        assert!(out.contains("<rect"), "svg child was stripped: {out}");
        assert!(out.contains("viewBox=\"0 0 10 10\""), "{out}");

        let hostile = clean(
            r#"<svg onload="alert(1)"><script>alert(1)</script><foreignObject><img src=x onerror=alert(1)></foreignObject><a xlink:href="javascript:alert(1)"><circle r="1"/></a></svg>"#,
        );
        assert!(!hostile.contains("onload"), "{hostile}");
        assert!(!hostile.contains("onerror"), "{hostile}");
        assert!(!hostile.contains("script"), "{hostile}");
        assert!(!hostile.contains("foreignObject"), "{hostile}");
        assert!(!hostile.contains("javascript:"), "{hostile}");
    }

    #[test]
    fn the_video_facade_survives_and_carries_nothing_else() {
        let out = clean(&format!(
            r#"<figure class="otw-video"><a href="https://www.youtube.com/watch?v=dQw4w9WgXcQ"><img src="{PNG}" alt="Talk"></a><figcaption>Talk</figcaption></figure>"#
        ));
        assert!(out.contains(r#"<figure class="otw-video">"#), "{out}");
        assert!(out.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), "{out}");
        assert!(out.contains(PNG), "{out}");
        assert!(out.contains("<figcaption>Talk</figcaption>"), "{out}");
        // A link out of the doc always opens detached from this origin.
        assert!(out.contains(r#"rel="noopener noreferrer nofollow""#), "{out}");

        // No iframe is part of the design, and none is allowed in.
        let framed = clean(r#"<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>"#);
        assert!(!framed.contains("iframe"), "{framed}");
        // Any other class on the figure is dropped.
        let classed = clean(r#"<figure class="otw-video evil"><figcaption>x</figcaption></figure>"#);
        assert!(classed.contains(r#"class="otw-video""#), "{classed}");
        assert!(!classed.contains("evil"), "{classed}");
    }

    #[test]
    fn scripts_and_handlers_never_reach_the_stored_body() {
        let out = clean(
            r#"<p onclick="alert(1)">hi</p><script>alert(1)</script><img src="x" onerror="alert(1)"><a href="javascript:alert(1)">go</a>"#,
        );
        assert!(!out.contains("onclick"), "{out}");
        assert!(!out.contains("onerror"), "{out}");
        assert!(!out.contains("alert"), "{out}");
        assert!(!out.contains("javascript:"), "{out}");
    }

    #[test]
    fn a_submission_must_be_self_contained_before_it_is_relayed() {
        let ok_html = format!(r#"<p><img src="{PNG}"></p>"#);
        let ok_json = json!({
            "type": "doc",
            "content": [{ "type": "image", "attrs": { "src": PNG } }]
        });
        assert_eq!(first_non_self_contained_src(&ok_html, &ok_json), None);

        // A remote image: nothing to publish, and a beacon if it were.
        assert_eq!(
            first_non_self_contained_src(r#"<img src="https://cdn.test/a.png">"#, &Value::Null),
            Some("https://cdn.test/a.png".to_string())
        );
        // An upload the inliner could not resolve (missing file, or not an image).
        assert_eq!(
            first_non_self_contained_src(
                &format!("<img src=\"{FILE_SRC_PREFIX}00000000-0000-0000-0000-000000000000\">"),
                &Value::Null
            ),
            Some(format!("{FILE_SRC_PREFIX}00000000-0000-0000-0000-000000000000"))
        );
        // A video embed's poster is checked like any other picture.
        let video = json!({
            "type": "doc",
            "content": [{
                "type": "videoEmbed",
                "attrs": { "provider": "youtube", "videoId": "dQw4w9WgXcQ", "poster": "/api/files/x" }
            }]
        });
        assert_eq!(
            first_non_self_contained_src("", &video),
            Some("/api/files/x".to_string())
        );
    }

    #[test]
    fn media_sources_are_visited_in_both_node_kinds() {
        let mut doc = json!({
            "type": "doc",
            "content": [
                { "type": "image", "attrs": { "src": "/api/files/a" } },
                { "type": "videoEmbed", "attrs": { "poster": "/api/files/b", "src": "ignored" } }
            ]
        });
        let mut seen = Vec::new();
        visit_media_srcs(&mut doc, &mut |s| {
            seen.push(s.clone());
            *s = format!("{s}!");
        });
        assert_eq!(seen, vec!["/api/files/a", "/api/files/b"]);
        assert_eq!(doc["content"][0]["attrs"]["src"], "/api/files/a!");
        assert_eq!(doc["content"][1]["attrs"]["poster"], "/api/files/b!");
        // A videoEmbed's `src`, if one ever appears, is not a media source.
        assert_eq!(doc["content"][1]["attrs"]["src"], "ignored");
    }
}
