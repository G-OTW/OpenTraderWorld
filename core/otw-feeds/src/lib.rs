//! News-feed fetchers: turn a configured feed + its secrets into normalized items.
//!
//! Two connectors:
//!   - `rss`: parse RSS/Atom with feed-rs.
//!   - `api`: GET/POST a JSON endpoint and extract fields via configurable paths.
//!
//! Secrets are referenced in the feed config as `{{secret:NAME}}` placeholders and
//! substituted just before the request. They are never logged.
//!
//! Errors caused by the *response shape* (bad JSON, an items_path that matches no
//! array, unparseable RSS) are prefixed with [`FORMAT_ERR_PREFIX`]. These reflect
//! feed configuration vs. the actual payload and are expected while a brand-new
//! feed is still being set up — the UI suppresses them until the first success.
//! "Hard" errors (network, HTTP status, missing url) are never prefixed.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context};
use otw_store::feeds::{Feed, NewItem};
use serde_json::Value;
use time::OffsetDateTime;

mod json_path;
use json_path::get_path;

/// Prefix marking an error as a response-shape/format problem (see module docs).
pub const FORMAT_ERR_PREFIX: &str = "format:";

/// Fetch and normalize all items currently available from a feed.
pub async fn fetch(feed: &Feed, secrets: &HashMap<String, String>) -> anyhow::Result<Vec<NewItem>> {
    match feed.kind.as_str() {
        "rss" => fetch_rss(feed).await,
        "api" => fetch_api(feed, secrets).await,
        other => Err(anyhow!("unknown feed kind: {other}")),
    }
}

fn client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("OpenTraderWorld/feeds")
        .timeout(Duration::from_secs(20))
        .build()
        .context("building http client")
}

// ── RSS / Atom ───────────────────────────────────────────────────────────────

async fn fetch_rss(feed: &Feed) -> anyhow::Result<Vec<NewItem>> {
    let url = feed
        .config
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("rss feed requires config.url"))?;

    let resp = client()?
        .get(url)
        .send()
        .await
        .context("rss request failed")?
        .error_for_status()
        .context("rss responded with error status")?;

    // Servers sometimes answer 200 with an HTML "rate limited" / error page (or a
    // login wall) instead of XML. Detect that early and report it as a hard error
    // with a useful snippet, rather than letting feed-rs emit a cryptic XML parse
    // error tagged as a format problem.
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = resp.bytes().await?;
    if let Some(msg) = non_feed_response(&content_type, &bytes) {
        return Err(anyhow!("{msg}"));
    }

    let parsed = feed_rs::parser::parse(&bytes[..])
        .map_err(|e| anyhow!("{FORMAT_ERR_PREFIX} parsing RSS/Atom: {e}"))?;
    let source_name = if feed.name.is_empty() {
        parsed.title.as_ref().map(|t| t.content.clone()).unwrap_or_default()
    } else {
        feed.name.clone()
    };

    let items = parsed
        .entries
        .into_iter()
        .map(|e| {
            let link = e.links.first().map(|l| l.href.clone());
            // Stable dedup key: prefer the entry id, fall back to the link.
            let dedup_key = if !e.id.is_empty() {
                e.id.clone()
            } else {
                link.clone().unwrap_or_else(|| e.title.as_ref().map(|t| t.content.clone()).unwrap_or_default())
            };
            NewItem {
                dedup_key,
                title: e.title.map(|t| t.content).unwrap_or_default(),
                url: link,
                summary: e.summary.map(|s| s.content).or_else(|| e.content.and_then(|c| c.body)),
                source_name: source_name.clone(),
                source_type: "rss".to_string(),
                published_at: e
                    .published
                    .or(e.updated)
                    .and_then(|dt| OffsetDateTime::from_unix_timestamp(dt.timestamp()).ok()),
                raw: None,
            }
        })
        .collect();
    Ok(items)
}

/// Detect a response that clearly isn't an RSS/Atom feed (an HTML error/login
/// page, or otherwise non-XML content) and return a human-readable error. Returns
/// `None` when the body plausibly is a feed, leaving parsing to feed-rs.
fn non_feed_response(content_type: &str, bytes: &[u8]) -> Option<String> {
    // Look at the start of the body, skipping a UTF-8 BOM and leading whitespace.
    let head = bytes.get(..bytes.len().min(512)).unwrap_or(bytes);
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let lower = trimmed.to_ascii_lowercase();

    let looks_xml = lower.starts_with("<?xml")
        || lower.starts_with("<rss")
        || lower.starts_with("<feed")
        || lower.starts_with("<rdf");
    if looks_xml {
        return None;
    }

    let looks_html = lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || content_type.contains("text/html");
    if looks_html {
        // Pull the <title> if present, for a more telling message.
        let title = extract_html_title(trimmed);
        return Some(match title {
            Some(t) => format!("expected an RSS/Atom feed but got an HTML page: {t}"),
            None => "expected an RSS/Atom feed but got an HTML page".to_string(),
        });
    }

    if trimmed.is_empty() {
        return Some("feed response was empty".to_string());
    }

    // Not XML, not HTML — let feed-rs try, but if the content-type is plainly
    // something else (JSON/plain text), say so.
    if content_type.contains("application/json") {
        return Some("expected an RSS/Atom feed but got JSON".to_string());
    }
    None
}

/// Best-effort extraction of an HTML document's <title> text (trimmed/clamped).
fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title>")? + open_end;
    let title = html.get(open_end..end)?.trim();
    if title.is_empty() {
        return None;
    }
    let clamped: String = title.chars().take(120).collect();
    Some(clamped)
}

// ── Generic JSON API ─────────────────────────────────────────────────────────

async fn fetch_api(feed: &Feed, secrets: &HashMap<String, String>) -> anyhow::Result<Vec<NewItem>> {
    let cfg = &feed.config;
    let url = cfg
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("api feed requires config.url"))?;
    let url = substitute(url, secrets);

    let method = cfg.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();

    let mut req = match method.as_str() {
        "POST" => client()?.post(&url),
        _ => client()?.get(&url),
    };

    // Headers (values may contain {{secret:...}}).
    if let Some(headers) = cfg.get("headers").and_then(Value::as_object) {
        for (k, v) in headers {
            if let Some(s) = v.as_str() {
                req = req.header(k, substitute(s, secrets));
            }
        }
    }
    // Query params.
    if let Some(query) = cfg.get("query").and_then(Value::as_object) {
        let pairs: Vec<(String, String)> = query
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), substitute(s, secrets))))
            .collect();
        req = req.query(&pairs);
    }

    let resp: Value = req
        .send()
        .await
        .context("api request failed")?
        .error_for_status()
        .context("api responded with error status")?
        .json()
        .await
        .map_err(|e| anyhow!("{FORMAT_ERR_PREFIX} parsing api JSON: {e}"))?;

    // Many APIs (Alpha Vantage, etc.) signal quota/auth problems with HTTP 200 and
    // an error message in the body, so error_for_status() above won't catch them.
    // Surface that real message as a *hard* error (not a "format:" one) — it's an
    // actionable problem, not a mapping mistake.
    if let Some(msg) = api_error_message(&resp) {
        return Err(anyhow!("{msg}"));
    }

    // Locate the array of items. Empty items_path => the response itself is the array.
    let items_path = cfg.get("items_path").and_then(Value::as_str).unwrap_or("");
    let arr = if items_path.is_empty() {
        resp.clone()
    } else {
        get_path(&resp, items_path).cloned().unwrap_or(Value::Null)
    };
    let arr = arr.as_array().ok_or_else(|| {
        anyhow!(
            "{FORMAT_ERR_PREFIX} items_path '{items_path}' did not resolve to an array \
             (response keys: {})",
            top_level_keys(&resp)
        )
    })?;

    let title_path = cfg.get("title_path").and_then(Value::as_str).unwrap_or("title");
    let url_path = cfg.get("url_path").and_then(Value::as_str).unwrap_or("url");
    let date_path = cfg.get("date_path").and_then(Value::as_str).unwrap_or("");
    let summary_path = cfg.get("summary_path").and_then(Value::as_str).unwrap_or("");
    let id_path = cfg.get("id_path").and_then(Value::as_str).unwrap_or("");
    let source_name = if feed.name.is_empty() { "API".to_string() } else { feed.name.clone() };

    let items = arr
        .iter()
        .map(|node| {
            let title = str_at(node, title_path);
            let url = str_at_opt(node, url_path);
            let summary = if summary_path.is_empty() { None } else { str_at_opt(node, summary_path) };
            let published_at = if date_path.is_empty() {
                None
            } else {
                str_at_opt(node, date_path).and_then(|s| parse_date(&s))
            };
            // dedup: explicit id_path, else url, else title.
            let dedup_key = if !id_path.is_empty() {
                str_at(node, id_path)
            } else {
                url.clone().unwrap_or_else(|| title.clone())
            };
            NewItem {
                dedup_key,
                title,
                url,
                summary,
                source_name: source_name.clone(),
                source_type: "api".to_string(),
                published_at,
                raw: Some(node.clone()),
            }
        })
        .collect();
    Ok(items)
}

/// Top-level object keys that conventionally carry an API error/notice message
/// when the endpoint returns HTTP 200 instead of a proper error status.
/// Matched case-insensitively. Covers Alpha Vantage ("Error Message", "Note",
/// "Information") and the common REST shapes ("error", "message", "detail").
const API_ERROR_KEYS: &[&str] = &[
    "error message",
    "error",
    "errors",
    "note",
    "information",
    "message",
    "detail",
    "fault",
];

/// If the response looks like an application-level error (an error key present,
/// or `status`/`success` indicating failure), return a human-readable message.
/// Returns `None` when the payload looks like normal data.
fn api_error_message(resp: &Value) -> Option<String> {
    let obj = resp.as_object()?;

    // An explicit `status: "error"` / `success: false` with no data array.
    let status_bad = obj
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("error") || s.eq_ignore_ascii_case("fail"));
    let success_false = obj.get("success").and_then(Value::as_bool) == Some(false);

    for (k, v) in obj {
        if API_ERROR_KEYS.iter().any(|e| k.eq_ignore_ascii_case(e)) {
            // Only treat it as an error if the value carries a message; an empty
            // "errors": [] array on an otherwise-valid payload isn't an error.
            if let Some(msg) = error_value_to_string(v) {
                return Some(format!("{k}: {msg}"));
            }
        }
    }

    if status_bad || success_false {
        return Some("API reported an error status".to_string());
    }
    None
}

/// Extract a readable message from an error field's value, or None if it's empty.
fn error_value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Array(a) if !a.is_empty() => Some(
            a.iter()
                .filter_map(|e| match e {
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        Value::Object(_) => Some(v.to_string()),
        _ => None,
    }
}

/// Comma-separated list of an object's top-level keys (for error context).
fn top_level_keys(v: &Value) -> String {
    match v.as_object() {
        Some(m) if !m.is_empty() => m.keys().cloned().collect::<Vec<_>>().join(", "),
        Some(_) => "(empty object)".to_string(),
        None if v.is_array() => "(response is an array)".to_string(),
        None => "(response is not an object)".to_string(),
    }
}

fn str_at(node: &Value, path: &str) -> String {
    str_at_opt(node, path).unwrap_or_default()
}
fn str_at_opt(node: &Value, path: &str) -> Option<String> {
    let v = get_path(node, path)?;
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parse common date formats: RFC3339, RFC2822, the Alpha Vantage compact form
/// (`YYYYMMDDThhmmss`, assumed UTC), and Unix epoch seconds.
fn parse_date(s: &str) -> Option<OffsetDateTime> {
    use time::format_description::well_known::{Rfc2822, Rfc3339};
    let s = s.trim();

    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(dt);
    }
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc2822) {
        return Some(dt);
    }

    // Alpha Vantage: "20240115T103000" (no zone) — treat as UTC.
    if let Some(dt) = parse_compact_utc(s) {
        return Some(dt);
    }

    // Unix epoch seconds as a string/number.
    if let Ok(secs) = s.parse::<i64>() {
        return OffsetDateTime::from_unix_timestamp(secs).ok();
    }
    None
}

/// Parse the zone-less compact timestamp `YYYYMMDDThhmmss` as a UTC instant.
fn parse_compact_utc(s: &str) -> Option<OffsetDateTime> {
    use time::format_description::BorrowedFormatItem;
    use time::PrimitiveDateTime;

    // Built once per call; cheap relative to the network fetch around it.
    let fmt: Vec<BorrowedFormatItem> =
        time::format_description::parse_borrowed::<2>("[year][month][day]T[hour][minute][second]")
            .ok()?;
    let naive = PrimitiveDateTime::parse(s, &fmt).ok()?;
    Some(naive.assume_utc())
}

/// Replace secret placeholders in a string with their values. Two forms are honored:
/// `{{secret:NAME}}` for per-feed secrets (map key = bare `NAME`), and `{{vault.item}}`
/// for inline vault references (map key already contains a `.`, matched verbatim).
/// Both are resolved into the same map upstream.
fn substitute(input: &str, secrets: &HashMap<String, String>) -> String {
    let mut out = input.to_string();
    for (name, value) in secrets {
        // Vault refs are keyed by their placeholder body ("vault.item"); per-feed
        // secrets are keyed by bare name and use the "secret:" prefix.
        if name.contains('.') {
            out = out.replace(&format!("{{{{{name}}}}}"), value);
        } else {
            out = out.replace(&format!("{{{{secret:{name}}}}}"), value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_alpha_vantage_rate_limit() {
        let note = json!({ "Note": "Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day." });
        let msg = api_error_message(&note).expect("should detect Note");
        assert!(msg.contains("Note"));
        assert!(msg.contains("rate limit"));

        let info = json!({ "Information": "The demo API key is for demo purposes only." });
        assert!(api_error_message(&info).is_some());

        let err = json!({ "Error Message": "the parameter apikey is invalid or missing." });
        assert!(api_error_message(&err).unwrap().contains("invalid or missing"));
    }

    #[test]
    fn ignores_normal_payload() {
        let ok = json!({ "items": "50", "feed": [{ "title": "Hi" }] });
        assert!(api_error_message(&ok).is_none());
        // An array response is normal data, not an error.
        assert!(api_error_message(&json!([{ "title": "x" }])).is_none());
    }

    #[test]
    fn detects_status_and_success_flags() {
        assert!(api_error_message(&json!({ "status": "error" })).is_some());
        assert!(api_error_message(&json!({ "success": false })).is_some());
        // Healthy flags don't trip it.
        assert!(api_error_message(&json!({ "status": "ok", "data": [] })).is_none());
    }

    #[test]
    fn empty_error_arrays_are_not_errors() {
        // A payload that happens to carry an empty `errors: []` alongside data.
        let v = json!({ "errors": [], "feed": [{ "title": "Hi" }] });
        assert!(api_error_message(&v).is_none());
    }

    #[test]
    fn xml_feeds_pass_through() {
        assert!(non_feed_response("application/rss+xml", b"<?xml version=\"1.0\"?><rss></rss>").is_none());
        assert!(non_feed_response("", b"<feed xmlns=\"...\"></feed>").is_none());
        // BOM + leading whitespace before the declaration.
        assert!(non_feed_response("", "\u{feff}\n  <?xml ?>".as_bytes()).is_none());
    }

    #[test]
    fn html_error_page_is_flagged_with_title() {
        let body = b"<!DOCTYPE html><html><head><title>429 Too Many Requests</title></head><body>x</body></html>";
        let msg = non_feed_response("text/html; charset=utf-8", body).expect("html flagged");
        assert!(msg.contains("HTML page"));
        assert!(msg.contains("429 Too Many Requests"));
    }

    #[test]
    fn json_and_empty_responses_are_flagged() {
        assert!(non_feed_response("application/json", b"{\"error\":\"nope\"}")
            .unwrap()
            .contains("JSON"));
        assert!(non_feed_response("", b"   ").unwrap().contains("empty"));
    }

    #[test]
    fn parses_alpha_vantage_compact_date() {
        let dt = parse_date("20240115T103000").expect("compact date");
        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month() as u8, 1);
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.hour(), 10);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.second(), 0);
        assert_eq!(dt.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn parses_rfc3339_rfc2822_and_epoch() {
        assert!(parse_date("2024-01-15T10:30:00Z").is_some());
        assert!(parse_date("Mon, 15 Jan 2024 10:30:00 +0000").is_some());
        assert_eq!(parse_date("0").unwrap(), OffsetDateTime::UNIX_EPOCH);
        assert!(parse_date("not a date").is_none());
    }
}
