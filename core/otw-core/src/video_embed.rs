//! Video embeds for documents: a canonical link plus one locally stored thumbnail.
//!
//! No video bytes are ever downloaded, stored or proxied. Resolving a video URL costs
//! exactly one poster image (a normal `files` row, like any pasted picture) and the
//! rendered block is a **facade**: poster + anchor, never an iframe. That choice is what
//! keeps the feature weightless and private:
//!
//!   * the doc body carries a link and a `data:` poster, so it stays self-contained and
//!     readable offline, the whole point of the Community Docs sync;
//!   * no third party is contacted when a reader opens a doc, so the website needs no
//!     `frame-src`/`media-src` loosening of its CSP and the app leaks nothing to Google.
//!
//! Security: the only caller-controlled value that ever reaches the network is the video
//! **id**, and it is pattern-validated before a URL is built. Hosts are compile-time
//! constants ([`POSTER_HOSTS`] and the per-provider oEmbed endpoints), redirects are not
//! followed, resolved addresses must be public, the download is byte-capped and the stored
//! bytes must sniff as a real raster image.

use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::Duration;

use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{link_preview, ApiError, AppState};

/// Hard cap on a poster download. A provider thumbnail is tens of kilobytes; anything
/// past this is not a thumbnail and is refused rather than stored.
const MAX_POSTER_BYTES: usize = 2 * 1024 * 1024;

/// Whole-request budget for each outbound call (oEmbed, then the poster).
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Hosts allowed to serve a poster. A provider hands back a `thumbnail_url`; it is data
/// from a third party, so it is checked against this list before it is fetched.
const POSTER_HOSTS: &[&str] = &[
    "i.ytimg.com",
    "img.youtube.com",
    "i.vimeocdn.com",
    "f.vimeocdn.com",
];

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/video-embed", post(resolve_embed))
}

// ── Providers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    YouTube,
    Vimeo,
}

impl Provider {
    fn id(self) -> &'static str {
        match self {
            Provider::YouTube => "youtube",
            Provider::Vimeo => "vimeo",
        }
    }
}

/// A parsed video reference: a provider and its id, nothing else. Everything sent to the
/// network is rebuilt from these two values, so a hostile URL cannot survive parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoRef {
    pub provider: Provider,
    pub id: String,
}

impl VideoRef {
    /// The canonical page a reader is sent to. Built here, never taken from the input.
    pub fn watch_url(&self) -> String {
        match self.provider {
            Provider::YouTube => format!("https://www.youtube.com/watch?v={}", self.id),
            Provider::Vimeo => format!("https://vimeo.com/{}", self.id),
        }
    }

    fn oembed_url(&self) -> String {
        let mut url = match self.provider {
            Provider::YouTube => reqwest::Url::parse("https://www.youtube.com/oembed").unwrap(),
            Provider::Vimeo => reqwest::Url::parse("https://vimeo.com/api/oembed.json").unwrap(),
        };
        url.query_pairs_mut()
            .append_pair("url", &self.watch_url())
            .append_pair("format", "json")
            // Vimeo sizes its thumbnail from this; without it the poster comes back 295px
            // wide and reads as a blur at the width the facade is rendered. YouTube ignores
            // it and always answers with the 480px hqdefault.
            .append_pair("width", "640");
        url.to_string()
    }

    /// Deterministic poster, used when oEmbed is unreachable. YouTube publishes one at a
    /// fixed path; Vimeo has no such URL, so a failed lookup there yields no poster.
    fn fallback_poster(&self) -> Option<String> {
        match self.provider {
            Provider::YouTube => Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", self.id)),
            Provider::Vimeo => None,
        }
    }
}

/// A YouTube id is exactly 11 URL-safe base64 characters.
fn is_youtube_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// A Vimeo id is a bare number. Length is bounded so a huge digit string cannot be used
/// to build an oversized URL.
fn is_vimeo_id(s: &str) -> bool {
    (6..=12).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// Recognise a video URL and reduce it to `(provider, id)`.
///
/// Only the shapes below are accepted; anything else is refused rather than guessed at,
/// because every later step trusts the id to be pattern-clean.
pub fn parse_video_url(raw: &str) -> Option<VideoRef> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();

    let youtube_id = match host.as_str() {
        "youtu.be" => segments.first().map(|s| s.to_string()),
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            match segments.as_slice() {
                // /watch?v=ID
                ["watch"] => url
                    .query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.into_owned()),
                // /embed/ID, /shorts/ID, /live/ID, /v/ID
                [kind, id, ..] if matches!(*kind, "embed" | "shorts" | "live" | "v") => {
                    Some((*id).to_string())
                }
                _ => None,
            }
        }
        _ => None,
    };
    if let Some(id) = youtube_id {
        return is_youtube_id(&id).then(|| VideoRef { provider: Provider::YouTube, id });
    }

    if host == "vimeo.com" || host == "player.vimeo.com" {
        // /ID, /channels/<name>/ID, /groups/<name>/videos/ID, /video/ID
        let id = segments.last().copied().unwrap_or_default();
        return is_vimeo_id(id).then(|| VideoRef { provider: Provider::Vimeo, id: id.to_string() });
    }

    None
}

// ── Outbound fetching ────────────────────────────────────────────────────────

/// Dedicated client: bounded, and it never follows a redirect. A 3xx is the one hop where
/// a fixed-host allowlist could be walked off, so it is refused instead of chased.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("OpenTraderWorld")
            .build()
            .unwrap_or_default()
    })
}

/// Refuse a host that resolves anywhere inside the local network. The hosts here are
/// constants, so this is defence in depth against a poisoned resolver rather than the
/// primary control (which is the allowlist itself).
async fn assert_public_host(url: &str) -> Result<(), ApiError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| ApiError::bad_request("invalid video URL"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ApiError::bad_request("invalid video URL"))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| {
            tracing::warn!("video embed: cannot resolve {host}: {e}");
            ApiError::bad_gateway("could not reach the video provider")
        })?
        .collect();
    if addrs.is_empty() {
        return Err(ApiError::bad_gateway("could not reach the video provider"));
    }
    // Every answer is checked, not just the one that would be used.
    if addrs.iter().any(|a| is_internal(a.ip())) {
        tracing::warn!("video embed: {host} resolves to an internal address, refusing");
        return Err(ApiError::bad_gateway("could not reach the video provider"));
    }
    Ok(())
}

/// Addresses this module may never reach. Mirrors the automator's outbound guard.
fn is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // Carrier-grade NAT, 100.64.0.0/10.
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_internal(IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00
                || (seg[0] & 0xffc0) == 0xfe80
        }
    }
}

/// A poster URL a provider handed back is only usable if it is https and served by a host
/// on the allowlist. Returns the normalised URL.
fn check_poster_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    POSTER_HOSTS
        .contains(&host.as_str())
        .then(|| url.to_string())
}

/// oEmbed lookup: the provider's own metadata endpoint. Best effort: a failure costs the
/// title and (on YouTube) falls back to the deterministic poster path.
async fn fetch_oembed(vref: &VideoRef) -> Option<(Option<String>, Option<String>)> {
    let url = vref.oembed_url();
    assert_public_host(&url).await.ok()?;
    let res = crate::rate::send(vref.provider.id(), client().get(&url)).await.ok()?;
    if !res.status().is_success() {
        tracing::info!("video embed: oembed returned HTTP {}", res.status().as_u16());
        return None;
    }
    let body: Value = res.json().await.ok()?;
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.trim().chars().take(300).collect::<String>())
        .filter(|t| !t.is_empty());
    let poster = body
        .get("thumbnail_url")
        .and_then(Value::as_str)
        .and_then(check_poster_url);
    Some((title, poster))
}

/// Download a poster and store it as an ordinary upload, returning `/api/files/{id}`.
///
/// The response is read with a running cap (never `bytes()` on an unbounded body) and the
/// result must sniff as a raster image, so a provider cannot hand back markup that later
/// renders in our origin.
async fn store_poster(state: &AppState, url: &str) -> Result<Option<String>, ApiError> {
    assert_public_host(url).await?;
    let Ok(res) = crate::rate::send("video-poster", client().get(url)).await else {
        return Ok(None);
    };
    if !res.status().is_success() {
        return Ok(None);
    }

    let mut res = res;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        match res.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len() + chunk.len() > MAX_POSTER_BYTES {
                    tracing::warn!("video embed: poster over {MAX_POSTER_BYTES} bytes, dropped");
                    return Ok(None);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("video embed: poster download failed: {e}");
                return Ok(None);
            }
        }
    }

    // The bytes decide, not the provider's header: only a real raster image is stored.
    let Some(content_type) = link_preview::sniff(&bytes) else {
        tracing::warn!("video embed: poster is not an image, dropped");
        return Ok(None);
    };

    let id = Uuid::new_v4();
    tokio::fs::write(state.upload_dir.join(id.to_string()), &bytes)
        .await
        .map_err(|e| anyhow::anyhow!("writing poster: {e}"))?;
    otw_store::files::record(
        &state.pool,
        id,
        "video-poster.jpg",
        content_type,
        bytes.len() as i64,
    )
    .await?;
    Ok(Some(format!("/api/files/{id}")))
}

// ── Route ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ResolveBody {
    /// The video page URL the user pasted.
    url: String,
}

/// POST /api/video-embed: turn a pasted video URL into `{provider, video_id, url, title,
/// poster}`. The poster is stored locally so the document keeps working offline and can be
/// relayed with the submission as a self-contained `data:` URI.
async fn resolve_embed(
    State(state): State<AppState>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(vref) = parse_video_url(&body.url) else {
        return Err(ApiError::bad_request(
            "unsupported video link, paste a YouTube or Vimeo URL",
        ));
    };

    let (title, poster_url) = match fetch_oembed(&vref).await {
        Some((title, poster)) => (title, poster.or_else(|| vref.fallback_poster())),
        None => (None, vref.fallback_poster()),
    };

    let poster = match poster_url {
        Some(url) => store_poster(&state, &url).await?,
        None => None,
    };

    Ok(Json(json!({
        "provider": vref.provider.id(),
        "video_id": vref.id,
        "url": vref.watch_url(),
        "title": title,
        "poster": poster,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(url: &str) -> Option<(String, String)> {
        parse_video_url(url).map(|v| (v.provider.id().to_string(), v.id))
    }

    #[test]
    fn youtube_urls_reduce_to_provider_and_id() {
        let want = Some(("youtube".to_string(), "dQw4w9WgXcQ".to_string()));
        assert_eq!(parsed("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), want);
        assert_eq!(parsed("https://youtube.com/watch?v=dQw4w9WgXcQ&t=42"), want);
        assert_eq!(parsed("https://youtu.be/dQw4w9WgXcQ?t=42"), want);
        assert_eq!(parsed("https://www.youtube.com/embed/dQw4w9WgXcQ"), want);
        assert_eq!(parsed("https://www.youtube.com/shorts/dQw4w9WgXcQ"), want);
        assert_eq!(parsed("https://m.youtube.com/watch?v=dQw4w9WgXcQ"), want);
        assert_eq!(
            parsed("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"),
            want
        );
    }

    #[test]
    fn vimeo_urls_reduce_to_provider_and_id() {
        let want = Some(("vimeo".to_string(), "123456789".to_string()));
        assert_eq!(parsed("https://vimeo.com/123456789"), want);
        assert_eq!(parsed("https://vimeo.com/channels/staffpicks/123456789"), want);
        assert_eq!(parsed("https://player.vimeo.com/video/123456789"), want);
    }

    #[test]
    fn anything_else_is_refused_rather_than_guessed() {
        assert_eq!(parsed("https://example.com/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(parsed("javascript:alert(1)"), None);
        assert_eq!(parsed("file:///etc/passwd"), None);
        // Wrong id shape: 10 chars, and a character outside the alphabet.
        assert_eq!(parsed("https://youtu.be/dQw4w9WgXc"), None);
        assert_eq!(parsed("https://youtu.be/dQw4w9WgXc%2F"), None);
        assert_eq!(parsed("https://vimeo.com/12"), None);
        assert_eq!(parsed("https://vimeo.com/not-a-number"), None);
        // A lookalike host must not be read as the real one.
        assert_eq!(parsed("https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(parsed("https://evilvimeo.com/123456789"), None);
    }

    #[test]
    fn outbound_urls_are_rebuilt_from_the_id_only() {
        let v = parse_video_url("https://youtu.be/dQw4w9WgXcQ?t=1&list=evil").unwrap();
        assert_eq!(v.watch_url(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert!(v.oembed_url().starts_with("https://www.youtube.com/oembed?url="));
        assert!(!v.oembed_url().contains("list"));
        assert_eq!(
            v.fallback_poster().as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );

        let v = parse_video_url("https://player.vimeo.com/video/123456789").unwrap();
        assert_eq!(v.watch_url(), "https://vimeo.com/123456789");
        assert!(v.oembed_url().starts_with("https://vimeo.com/api/oembed.json?url="));
        assert_eq!(v.fallback_poster(), None);
    }

    #[test]
    fn a_poster_url_is_only_taken_from_an_allowlisted_host() {
        assert!(check_poster_url("https://i.ytimg.com/vi/x/hqdefault.jpg").is_some());
        assert!(check_poster_url("https://i.vimeocdn.com/video/1_640.jpg").is_some());
        // Not https, not on the list, and not a URL at all.
        assert!(check_poster_url("http://i.ytimg.com/vi/x/hqdefault.jpg").is_none());
        assert!(check_poster_url("https://evil.test/x.jpg").is_none());
        assert!(check_poster_url("https://i.ytimg.com.evil.test/x.jpg").is_none());
        assert!(check_poster_url("file:///etc/passwd").is_none());
        assert!(check_poster_url("not a url").is_none());
    }

    #[test]
    fn internal_addresses_are_never_reachable() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.5",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_internal(ip.parse().unwrap()), "{ip} should be internal");
        }
        for ip in ["1.1.1.1", "142.250.74.206", "2606:4700::1111"] {
            assert!(!is_internal(ip.parse().unwrap()), "{ip} should be public");
        }
    }
}

/// Live checks against the two providers. Ignored by default so the normal test run stays
/// offline; run with `cargo test -p otw-core -- --ignored video_embed`.
#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits youtube.com and vimeo.com"]
    async fn oembed_answers_with_a_title_and_an_allowlisted_poster() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://vimeo.com/22439234",
        ] {
            let vref = parse_video_url(url).unwrap();
            let (title, poster) = fetch_oembed(&vref)
                .await
                .unwrap_or_else(|| panic!("{url}: oembed lookup failed"));
            assert!(title.is_some(), "{url}: no title");
            let poster = poster.unwrap_or_else(|| panic!("{url}: poster host was refused"));

            // The poster has to be a real image, or `store_poster` would drop it.
            let res = crate::rate::send("test", client().get(&poster)).await.unwrap();
            assert!(res.status().is_success(), "{url}: poster HTTP {}", res.status());
            let bytes = res.bytes().await.unwrap();
            assert_eq!(link_preview::sniff(&bytes), Some("image/jpeg"), "{url}");
            assert!(bytes.len() < MAX_POSTER_BYTES, "{url}: poster over the cap");
        }
    }
}
