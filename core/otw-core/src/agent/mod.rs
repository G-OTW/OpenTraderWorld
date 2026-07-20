//! Agent module — a light, provider-agnostic chat agent inside OTW.
//!
//! Phases 0–1 (chat core): a `Provider` trait with two wire-format adapters (Anthropic
//! Messages + OpenAI-compatible `/chat/completions`), a server-side streaming run loop, and
//! the storage/API to back a ChatGPT-like chat. Tools/MCP (Phase 2) and memory/skills
//! (Phase 3) plug into the seams here without a rewrite.
//!
//! No vendor is privileged anywhere: the active provider + model come entirely from user
//! config. The two adapters exist for wire-format reasons only.

pub mod anthropic;
pub mod mcp_client;
pub mod openai_compat;
pub mod provider;
pub mod run;
mod sse;
pub mod summary;
pub mod tools;

use provider::Provider;

/// Shared HTTP client for all provider calls: connection reuse plus a connect timeout so a
/// dead upstream fails fast. Deliberately NO overall request timeout — streams run long;
/// mid-stream stalls are handled by the run loop's idle timeout.
pub(crate) fn http() -> &'static reqwest::Client {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("building provider http client")
    })
}

/// Params that steer OTW itself and must never reach a provider's wire (unknown top-level
/// fields are a hard error on some APIs).
const INTERNAL_PARAMS: &[&str] = &["summary_max_tokens"];

/// Merge the agent's user-defined `params` into a request body: every non-internal key is
/// copied verbatim (overriding the adapter's defaults), and an explicit `null` REMOVES the
/// key — e.g. `{"max_completion_tokens": 4096, "max_tokens": null}` for newer OpenAI models,
/// or `{"stream_options": null}` for compat providers that reject it.
pub(crate) fn apply_params(body: &mut serde_json::Value, params: &serde_json::Value) {
    let (Some(map), Some(p)) = (body.as_object_mut(), params.as_object()) else { return };
    for (k, v) in p {
        if INTERNAL_PARAMS.contains(&k.as_str()) {
            continue;
        }
        if v.is_null() {
            map.remove(k);
        } else {
            map.insert(k.clone(), v.clone());
        }
    }
}

/// Build the right adapter for a stored provider row + its (secret) key.
pub fn build_provider(kind: &str, base_url: &str, api_key: String) -> Option<Box<dyn Provider>> {
    match kind {
        "anthropic" => Some(Box::new(anthropic::Anthropic {
            base_url: base_url.to_string(),
            api_key,
        })),
        "openai_compat" => Some(Box::new(openai_compat::OpenAiCompat {
            base_url: base_url.to_string(),
            api_key,
        })),
        _ => None,
    }
}

/// Bare host of a URL, for rate-dashboard grouping.
pub(crate) fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?'])
        .next()
        .unwrap_or("provider")
        .to_string()
}

/// Normalize a memory slug: lowercase, keep [a-z0-9-], collapse other runs to a single '-'.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Read a `Retry-After` header (integer seconds) from a provider response, if present.
pub(crate) fn retry_after(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// Turn a provider's HTTP error status + body into a short, user-facing message. Keeps the
/// operator out of the raw JSON: rate limits, auth failures, and refusals get a plain sentence;
/// everything else falls back to the status plus a trimmed body snippet.
pub(crate) fn friendly_http_error(status: u16, body: &str) -> String {
    // Try to lift a provider-supplied message (both wire formats nest it under "error").
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let detail = detail.trim();
    match status {
        401 | 403 => "Provider rejected the API key (401/403). Check the key in agent settings.".to_string(),
        429 => {
            if detail.is_empty() {
                "Provider rate limit hit (429). Wait a moment and try again.".to_string()
            } else {
                format!("Provider rate limit hit (429): {}", truncate(detail, 200))
            }
        }
        404 => format!(
            "Provider returned 404 — the model name or base URL is likely wrong{}.",
            if detail.is_empty() { String::new() } else { format!(" ({})", truncate(detail, 160)) }
        ),
        400 | 422 => format!(
            "Provider rejected the request ({status}): {}",
            if detail.is_empty() { "bad request".to_string() } else { truncate(detail, 240) }
        ),
        500..=599 => "Provider is having trouble (5xx). Try again shortly.".to_string(),
        _ => {
            let snippet = if detail.is_empty() { truncate(body, 240) } else { truncate(detail, 240) };
            format!("Provider error HTTP {status}: {snippet}")
        }
    }
}

/// Truncate to at most `n` bytes on a char boundary, no ellipsis (for identifiers).
pub(crate) fn truncate_plain(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut end = n;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
