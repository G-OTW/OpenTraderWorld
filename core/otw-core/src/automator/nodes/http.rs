//! `http`: call something outside OpenTraderWorld.
//!
//! The whole point of the block is that the user decides what to call, so the guard here is
//! not a URL allowlist but a network one, and it is opt-out per node rather than global:
//!
//! 1. The host is resolved **first**, and every resolved address is checked. Loopback,
//!    private, carrier-grade NAT, link-local (including the cloud metadata address) and
//!    unique-local IPv6 are refused unless the node sets `allow_internal`, which is how
//!    "call my own API" or "call the box on my LAN" stays possible, knowingly.
//! 2. The connection is then **pinned** to the address that was checked, so a name that
//!    answers differently a millisecond later (DNS rebinding) cannot move the target.
//! 3. Redirects are followed by hand, at most three, re-running both checks per hop.
//!
//! Responses are read with a byte cap and a payload over the inline limit is handed to the
//! engine as bytes, which stores it as a blob and passes a handle downstream.

use std::net::{IpAddr, SocketAddr};

use serde_json::{json, Value};

use super::{NodeCtx, Outcome};

pub const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];
pub const PARSERS: &[&str] = &["json", "text", "csv", "binary"];

/// Default and maximum response size the node will read.
const DEFAULT_MAX_BYTES: usize = 1024 * 1024;
const HARD_MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const DEFAULT_TIMEOUT: u64 = 30;

pub fn validate(config: &Value) -> Result<(), String> {
    let method = super::opt(config, "method", "GET").to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(format!("unsupported method \"{method}\""));
    }
    let url = super::field(config, "url")?;
    // A template can produce anything, so only a literal URL is checked at save time.
    if !url.contains("{{") && !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("the URL must start with http:// or https://".into());
    }
    let parse = super::opt(config, "parse", "json");
    if !PARSERS.contains(&parse) {
        return Err(format!("unknown response format \"{parse}\""));
    }
    if config.get("max_bytes").and_then(|v| v.as_u64()).is_some_and(|b| b as usize > HARD_MAX_BYTES)
    {
        return Err(format!("max_bytes caps at {} KB", HARD_MAX_BYTES / 1024));
    }
    // A literal JSON body is checked now; one with a template in it can only be checked
    // once the template is resolved, which the block does at run time.
    if super::opt(config, "body_kind", "none") == "json" {
        if let Some(text) = config.get("body").and_then(|v| v.as_str()) {
            if !text.trim().is_empty() && !text.contains("{{") {
                serde_json::from_str::<Value>(text)
                    .map_err(|e| format!("the JSON body is not valid JSON: {e}"))?;
            }
        }
    }
    for key in ["headers", "query"] {
        if let Some(list) = config.get(key) {
            if !list.is_array() {
                return Err(format!("{key} must be a list of name/value pairs"));
            }
            if list.as_array().is_some_and(|a| a.len() > 30) {
                return Err(format!("at most 30 {key} entries"));
            }
        }
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let config = &ctx.node.config;
    let method = super::opt(config, "method", "GET").to_ascii_uppercase();
    let allow_internal = config.get("allow_internal").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_bytes = config
        .get("max_bytes")
        .and_then(|v| v.as_u64())
        .map(|b| (b as usize).min(HARD_MAX_BYTES))
        .unwrap_or(DEFAULT_MAX_BYTES);

    // The URL itself may not carry a secret: a template there decides the host, and a
    // vault reference would pick where the secret is sent. A query value may, because the
    // `?api_key=` API is too common to refuse, but it is the caller's own exposure: the
    // target writes it to its access log. Ours does not, provided the percent-encoded form
    // is registered as a secret too, since the raw one no longer appears in the URL.
    let mut url = ctx.resolver.render_str(super::field(config, "url")?, ctx.vars, false).await?;
    let query = pairs(ctx, config, "query", true).await?;
    if !query.is_empty() {
        let encoded: Vec<String> = query
            .iter()
            .map(|(k, v)| {
                let cooked = urlencode(v);
                if ctx.resolver.secrets.scrub(v) != *v {
                    ctx.resolver.secrets.add(&cooked);
                }
                format!("{}={cooked}", urlencode(k))
            })
            .collect();
        let sep = if url.contains('?') { '&' } else { '?' };
        url = format!("{url}{sep}{}", encoded.join("&"));
    }
    let headers = pairs(ctx, config, "headers", true).await?;
    let body = body_of(ctx, config).await?;

    let request = json!({
        "method": method,
        "url": url,
        "headers": headers.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>(),
        "body": body.clone().map(Value::String).unwrap_or(Value::Null),
        "allow_internal": allow_internal,
    });

    if let crate::automator::RunMode::Test { call_external, .. } = ctx.mode {
        if !call_external || method != "GET" {
            return Ok(Outcome::simulated(
                request,
                if call_external {
                    "test run: only GET calls are executed"
                } else {
                    "test run: external calls are off"
                },
            ));
        }
    }

    let timeout = ctx.budget(DEFAULT_TIMEOUT);
    let mut current = url.clone();
    let mut hops = 0usize;
    loop {
        let addr = resolve(&current, allow_internal).await?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .resolve(host_of(&current)?.as_str(), addr)
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let mut builder = client.request(
            reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "bad method")?,
            &current,
        );
        for (k, v) in &headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if let Some(b) = &body {
            builder = builder.body(b.clone());
        }
        let resp = crate::rate::send("automator", builder)
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        if status.is_redirection() && hops < MAX_REDIRECTS {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("redirect without a location")?;
            current = join_url(&current, location)?;
            hops += 1;
            continue;
        }

        let ctype = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let resp_headers = collect_headers(&resp);
        let bytes = read_capped(resp, max_bytes).await?;

        let parse = super::opt(config, "parse", "json");
        let output = parse_body(parse, &bytes, &ctype, status.as_u16(), resp_headers)?;
        return Ok(Outcome::new(request, output));
    }
}

/// Resolve a URL's host and return the address the request will be pinned to.
async fn resolve(url: &str, allow_internal: bool) -> Result<SocketAddr, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("only http:// and https:// URLs can be called".into());
    }
    let host = parsed.host_str().ok_or("the URL has no host")?.to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .collect();
    let Some(addr) = addrs.first().copied() else {
        return Err(format!("{host} resolved to no address"));
    };
    if !allow_internal {
        // Every answer is checked, not just the one used: a name that resolves to a public
        // and a private address must not become a way in.
        for a in &addrs {
            if is_internal(a.ip()) {
                return Err(format!(
                    "{host} resolves to the internal address {}. Switch on \"allow internal \
                     targets\" on this block if that is what you meant",
                    a.ip()
                ));
            }
        }
    }
    Ok(addr)
}

/// Addresses a workflow may not reach by default.
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
                // 0.0.0.0/8.
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
                // Unique local fc00::/7 and link-local fe80::/10.
                || (seg[0] & 0xfe00) == 0xfc00
                || (seg[0] & 0xffc0) == 0xfe80
        }
    }
}

fn host_of(url: &str) -> Result<String, String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .ok_or_else(|| "invalid URL".to_string())
}

fn join_url(base: &str, location: &str) -> Result<String, String> {
    let base = reqwest::Url::parse(base).map_err(|e| format!("invalid URL: {e}"))?;
    base.join(location).map(|u| u.to_string()).map_err(|e| format!("bad redirect: {e}"))
}

fn collect_headers(resp: &reqwest::Response) -> Value {
    let mut out = serde_json::Map::new();
    for (name, value) in resp.headers() {
        if let Ok(v) = value.to_str() {
            out.insert(name.as_str().to_string(), Value::String(v.to_string()));
        }
    }
    Value::Object(out)
}

/// Read a response body, refusing to grow past the cap instead of buffering it whole.
async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("reading response: {e}"))? {
        if out.len() + chunk.len() > cap {
            return Err(format!(
                "the response is larger than the {} KB limit of this block",
                cap / 1024
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn parse_body(
    parse: &str,
    bytes: &[u8],
    content_type: &str,
    status: u16,
    headers: Value,
) -> Result<Value, String> {
    let body = match parse {
        "json" => {
            let text = String::from_utf8_lossy(bytes);
            serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text.to_string()))
        }
        "binary" => json!({
            "_bytes": bytes.len(),
            "content_type": content_type,
            "_base64": base64_of(bytes),
        }),
        _ => Value::String(String::from_utf8_lossy(bytes).to_string()),
    };
    Ok(json!({ "status": status, "headers": headers, "body": body }))
}

/// Binary bodies travel as base64 so a later block can post them on; the engine parks
/// anything oversized in a blob before it reaches a step output.
fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Render a list of `{name, value}` pairs.
async fn pairs(
    ctx: &NodeCtx<'_>,
    config: &Value,
    key: &str,
    allow_secrets: bool,
) -> Result<Vec<(String, String)>, String> {
    let list = config.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::with_capacity(list.len());
    for item in &list {
        let name = super::opt(item, "name", "");
        if name.is_empty() {
            continue;
        }
        let value = ctx
            .resolver
            .render_str(item.get("value").and_then(|v| v.as_str()).unwrap_or(""), ctx.vars, allow_secrets)
            .await?;
        out.push((name.to_string(), value));
    }
    Ok(out)
}

async fn body_of(ctx: &NodeCtx<'_>, config: &Value) -> Result<Option<String>, String> {
    match super::opt(config, "body_kind", "none") {
        "json" => {
            let raw = config.get("body").cloned().unwrap_or(Value::Null);
            let rendered = match &raw {
                // The editor's body field is a text area, so the usual shape here is a
                // *string* holding JSON. Serializing that as it stands would put a quoted
                // string on the wire instead of an object, so it is parsed back once the
                // expressions inside it are resolved.
                Value::String(text) => {
                    let filled = ctx.resolver.render_str(text, ctx.vars, true).await?;
                    if filled.trim().is_empty() {
                        return Ok(None);
                    }
                    serde_json::from_str(&filled).map_err(|e| {
                        format!(
                            "the JSON body is not valid JSON once its expressions are \
                             resolved: {e}"
                        )
                    })?
                }
                other => ctx.resolver.render_value(other, ctx.vars, true).await?,
            };
            Ok(Some(serde_json::to_string(&rendered).unwrap_or_default()))
        }
        "text" => {
            let tpl = config.get("body").and_then(|v| v.as_str()).unwrap_or("");
            Ok(Some(ctx.resolver.render_str(tpl, ctx.vars, true).await?))
        }
        "form" => {
            let list = pairs(ctx, config, "form", true).await?;
            Ok(Some(
                list.iter()
                    .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                    .collect::<Vec<_>>()
                    .join("&"),
            ))
        }
        _ => Ok(None),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(body_kind: &str, body: &str) -> Value {
        json!({ "url": "https://x.test", "body_kind": body_kind, "body": body })
    }

    #[test]
    fn a_literal_json_body_that_is_not_json_is_refused_at_save_time() {
        let err = validate(&cfg("json", "{ nope }")).unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
    }

    #[test]
    fn a_json_body_holding_a_template_waits_for_run_time() {
        // `{{steps.a.output}}` where a number goes is not JSON yet; it is once resolved.
        validate(&cfg("json", "{\"qty\": {{steps.a.output.qty}} }")).expect("deferred");
    }

    #[test]
    fn a_text_body_is_never_parsed_as_json() {
        validate(&cfg("text", "hello, not json")).expect("text is free-form");
    }
}
