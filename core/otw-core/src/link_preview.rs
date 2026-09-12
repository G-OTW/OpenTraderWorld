//! Thumbnail discovery for a user-supplied link.
//!
//! Given a URL, fetch the page and pull out the image a social card would use:
//! `og:image` → `twitter:image` → `apple-touch-icon` → `<link rel=icon>` → `/favicon.ico`.
//! The first candidate that downloads as a real image wins; the caller stores the bytes.
//! No headless browser, so no screenshot of the rendered page — only what the page declares.
//!
//! Everything here fetches a URL the user typed, so the outbound side is deliberately
//! narrow:
//!   * http/https only, and **no automatic redirects** — each hop is re-checked by hand.
//!   * Every host is resolved before the request and refused when it lands on a private,
//!     loopback, link-local or otherwise internal address (SSRF). Note the request
//!     re-resolves the name, so a DNS entry that flips between two answers is not fully
//!     defeated; that residual is accepted for a single-user self-hosted app, and the
//!     route is denied outright in demo mode.
//!   * Hard caps on body size and time, so a hostile page cannot stream forever.
//!   * SVG is rejected: the bytes end up served from our own origin, where a scripted SVG
//!     opened directly would run as us. Raster formats only.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;

use crate::ApiError;

/// Enough for any real `<head>`; the scan stops there anyway.
const MAX_HTML: usize = 512 * 1024;
/// A social card image; anything larger is not a thumbnail.
const MAX_IMAGE: usize = 4 * 1024 * 1024;
const MAX_HOPS: usize = 4;
const TIMEOUT: Duration = Duration::from_secs(8);

/// Plain browser UA on purpose: a bot token gets a 403 from a good share of sites (and
/// from most CDNs), which would make the feature look broken rather than polite.
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                  Chrome/126.0.0.0 Safari/537.36";

pub struct Image {
    pub bytes: Vec<u8>,
    pub content_type: String,
    /// Absolute URL the bytes came from — kept for the filename and for logs.
    pub source: String,
}

/// Find a thumbnail for `raw`. `Ok(None)` = the page was read but declares no usable image.
pub async fn thumbnail(raw: &str) -> Result<Option<Image>, ApiError> {
    let page = normalize(raw)?;
    let client = client()?;

    let (final_url, body, ctype) = get(&client, page, MAX_HTML).await?;

    // A link straight to an image is its own thumbnail — re-fetched whole when the page
    // cap cut it short.
    if sniff(&body).is_some() {
        if body.len() < MAX_HTML {
            if let Some(img) = as_image(&final_url, &body, &ctype) {
                return Ok(Some(img));
            }
        }
        return Ok(fetch_image(&client, final_url).await);
    }

    let html = String::from_utf8_lossy(&body);
    let mut candidates = declared_images(&html);
    // Last resort, and the reason a plain site with no card still gets an icon.
    candidates.push("/favicon.ico".to_string());

    for cand in candidates {
        let Ok(url) = final_url.join(&cand) else { continue };
        if let Some(img) = fetch_image(&client, url).await {
            return Ok(Some(img));
        }
    }
    Ok(None)
}

/// One candidate: fetch it, and keep it only if it is a whole, real image.
async fn fetch_image(client: &reqwest::Client, url: Url) -> Option<Image> {
    let (u, bytes, ct) = get(client, url, MAX_IMAGE).await.ok()?;
    if bytes.len() >= MAX_IMAGE {
        return None; // cut at the cap — a truncated image is not worth storing
    }
    as_image(&u, &bytes, &ct)
}

fn client() -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .user_agent(UA)
        .build()
        .map_err(|e| ApiError::internal(&format!("http client: {e}")))
}

/// Accept what a user pastes: bare hosts get https, anything not http(s) is refused.
fn normalize(raw: &str) -> Result<Url, ApiError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ApiError::bad_request("link required"));
    }
    let with_scheme =
        if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let url = Url::parse(&with_scheme).map_err(|_| ApiError::bad_request("invalid link"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::bad_request("only http(s) links can be previewed"));
    }
    Ok(url)
}

/// GET with manual redirect handling and a hard byte cap, returning the final URL.
async fn get(
    client: &reqwest::Client,
    mut url: Url,
    cap: usize,
) -> Result<(Url, Vec<u8>, String), ApiError> {
    for _ in 0..MAX_HOPS {
        guard(&url).await?;
        let res = client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| ApiError::bad_gateway(&format!("could not reach the site: {e}")))?;

        if res.status().is_redirection() {
            let next = res
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url.join(loc).ok())
                .ok_or_else(|| ApiError::bad_gateway("redirect without a usable target"))?;
            url = next;
            continue;
        }
        if !res.status().is_success() {
            return Err(ApiError::bad_gateway(&format!("site answered {}", res.status())));
        }

        let ctype = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Stream so an unbounded (or lying) response is cut at the cap instead of buffered.
        let mut res = res;
        let mut body = Vec::new();
        while let Some(chunk) = res
            .chunk()
            .await
            .map_err(|e| ApiError::bad_gateway(&format!("reading the page: {e}")))?
        {
            body.extend_from_slice(&chunk);
            if body.len() >= cap {
                body.truncate(cap);
                break;
            }
        }
        return Ok((url, body, ctype));
    }
    Err(ApiError::bad_gateway("too many redirects"))
}

// ── SSRF guard ───────────────────────────────────────────────────────────────

async fn guard(url: &Url) -> Result<(), ApiError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::bad_request("only http(s) links can be previewed"));
    }
    let host = url.host_str().ok_or_else(|| ApiError::bad_request("link has no host"))?;
    let port = url.port_or_known_default().unwrap_or(80);

    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ApiError::bad_gateway("could not resolve that host"))?;

    let mut any = false;
    for addr in addrs {
        any = true;
        if internal(addr.ip()) {
            return Err(ApiError::bad_request("that address is on a private network"));
        }
    }
    if !any {
        return Err(ApiError::bad_gateway("could not resolve that host"));
    }
    Ok(())
}

/// Addresses a link preview must never reach: our own host, the LAN, cloud metadata.
fn internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            v.is_loopback()
                || v.is_private()
                || v.is_link_local() // 169.254/16, incl. cloud metadata
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64/10 CGNAT
                || o[0] >= 240 // reserved
        }
        IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() {
                return internal(IpAddr::V4(m));
            }
            v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (v.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
                || (v.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
        }
    }
}

// ── Image acceptance ─────────────────────────────────────────────────────────

/// Keep the bytes only when they really are a raster image, whatever the header claimed.
fn as_image(url: &Url, bytes: &[u8], ctype: &str) -> Option<Image> {
    if bytes.len() < 64 {
        return None;
    }
    let sniffed = sniff(bytes)?;
    let declared = ctype.split(';').next().unwrap_or("").trim();
    // Servers mislabel icons constantly (`text/plain` for a .ico is routine), so the magic
    // number decides; the declared type only has to not contradict it with a non-image.
    if !declared.is_empty() && !declared.starts_with("image/") && !declared.contains("octet-stream")
    {
        return None;
    }
    Some(Image {
        bytes: bytes.to_vec(),
        content_type: sniffed.to_string(),
        source: url.to_string(),
    })
}

/// Magic-number image detection. Also the single source of truth for what `files.rs` will
/// serve inline from our own origin — a byte-sniffed raster type can be trusted in a
/// `Content-Type`, a caller-declared one cannot.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(b"RIFF") && b.len() > 12 && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("image/x-icon")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else if b.len() > 12 && &b[4..8] == b"ftyp" && (&b[8..12] == b"avif" || &b[8..12] == b"avis") {
        Some("image/avif")
    } else {
        None
    }
}

// ── Minimal HTML scan ────────────────────────────────────────────────────────

/// Image URLs the page declares, best first. Deliberately a tag scanner and not a parser:
/// we only need `<meta>`/`<link>` attributes out of the head, and a real DOM is a heavy
/// dependency for that.
fn declared_images(html: &str) -> Vec<String> {
    // The head is where cards live; stop there when it closes so a huge body costs nothing.
    let lower = html.to_ascii_lowercase();
    let end = lower.find("</head").unwrap_or(lower.len());
    let head = &html[..end];

    let mut og = Vec::new();
    let mut twitter = Vec::new();
    let mut apple = Vec::new();
    let mut icon = Vec::new();

    for tag in tags(head) {
        let get = |k: &str| tag.attr(k).map(str::to_string);
        match tag.name.as_str() {
            "meta" => {
                let key = get("property").or_else(|| get("name")).unwrap_or_default();
                let val = match get("content") {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => continue,
                };
                match key.to_ascii_lowercase().as_str() {
                    "og:image:secure_url" | "og:image:url" | "og:image" => og.push(val),
                    "twitter:image" | "twitter:image:src" => twitter.push(val),
                    _ => {}
                }
            }
            "link" => {
                let rel = get("rel").unwrap_or_default().to_ascii_lowercase();
                let href = match get("href") {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => continue,
                };
                if rel.contains("apple-touch-icon") {
                    apple.push(href);
                } else if rel.split_whitespace().any(|r| r == "icon" || r == "shortcut") {
                    icon.push(href);
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    for v in og.into_iter().chain(twitter).chain(apple).chain(icon) {
        let v = decode_entities(v.trim());
        if !v.is_empty() && !v.starts_with("data:") && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Every `<meta>` / `<link>` in `html`, with their attributes lowercased by name.
fn tags(html: &str) -> Vec<Tag> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = html[i..].find('<') {
        let start = i + off + 1;
        i = start;
        let rest = &html[start..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name != "meta" && name != "link" {
            continue;
        }
        // Attributes run to the first '>' that is not inside a quoted value.
        let attrs_at = start + name.len();
        let mut j = attrs_at;
        let (mut quote, mut end) = (None::<u8>, None::<usize>);
        while j < bytes.len() {
            let c = bytes[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == b'"' || c == b'\'' => quote = Some(c),
                None if c == b'>' => {
                    end = Some(j);
                    break;
                }
                None => {}
            }
            j += 1;
        }
        let Some(end) = end else { break };
        out.push(Tag { name, attrs: parse_attrs(&html[attrs_at..end]) });
        i = end + 1;
    }
    out
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < b.len() && !(b[i] as char).is_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if i == ks {
            i += 1;
            continue;
        }
        let key = s[ks..i].to_ascii_lowercase();
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            out.push((key, String::new()));
            continue;
        }
        i += 1;
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let val = if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            i += 1;
            let vs = i;
            while i < b.len() && b[i] != q {
                i += 1;
            }
            let v = s[vs..i.min(s.len())].to_string();
            i += 1;
            v
        } else {
            let vs = i;
            while i < b.len() && !(b[i] as char).is_whitespace() && b[i] != b'>' {
                i += 1;
            }
            s[vs..i].to_string()
        };
        out.push((key, val));
    }
    out
}

/// The handful of entities that actually show up inside a URL attribute.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
