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
pub mod builtin;
pub mod confirm;
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
const INTERNAL_PARAMS: &[&str] =
    &["summary_max_tokens", "summary_model", "summary_trigger_tokens", "prompt_cache"];

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

/// How many times a throttled or briefly-broken request is re-sent before giving up.
const MAX_RETRIES: u32 = 2;
/// Never wait longer than this on a `Retry-After`, whatever the provider asks for: past it
/// the user is better served by an error than by a chat that appears frozen.
const MAX_RETRY_WAIT: u64 = 20;

/// POST a provider request, retrying a 429 or a 5xx with the delay the provider asked for.
///
/// Worth the code because of where these land: a 429 on round 9 of a tool loop throws away
/// eight rounds that were already paid for. Only the statuses that mean "later, not never"
/// are retried; 4xx that describe the request itself come straight back.
pub(crate) async fn send_retrying(
    provider: &str,
    builder: reqwest::RequestBuilder,
) -> reqwest::Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        // try_clone fails only on a streaming body, which no provider call uses — without a
        // clone there is nothing to retry with, so send once and return whatever comes back.
        let Some(next) = builder.try_clone() else {
            return crate::rate::send(provider, builder).await;
        };
        let resp = crate::rate::send(provider, next).await?;
        let status = resp.status().as_u16();
        let retryable = status == 429 || (500..=599).contains(&status);
        if !retryable || attempt >= MAX_RETRIES {
            return Ok(resp);
        }
        // Honour Retry-After when it is short enough, else back off 2s then 6s.
        let wait = retry_after(&resp)
            .filter(|s| *s <= MAX_RETRY_WAIT)
            .unwrap_or(2 + attempt as u64 * 4);
        tracing::warn!("agent: provider {provider} returned {status}, retrying in {wait}s");
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        attempt += 1;
    }
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
        // A model that simply cannot call tools comes back as a 404 too, and "check the model
        // name" sends the user to look at a name that is perfectly correct.
        404 if detail.to_ascii_lowercase().contains("tool") => format!(
            "The model does not support tool calling ({}). Pick a tool-capable model to reach \
             your data.",
            truncate(detail, 200)
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A model with no tool support answers 404, which used to read as "check the model name"
    /// — sending the user to inspect an id that is perfectly correct.
    #[test]
    fn a_404_about_tools_names_the_real_cause() {
        let body = r#"{"error":{"message":"No endpoints found that support tool use."}}"#;
        let msg = friendly_http_error(404, body);
        assert!(msg.contains("does not support tool calling"), "{msg}");
        assert!(!msg.contains("base URL"), "{msg}");
        // A plain 404 still points at the name/URL.
        let other = friendly_http_error(404, r#"{"error":{"message":"model not found"}}"#);
        assert!(other.contains("base URL"), "{other}");
    }
}
