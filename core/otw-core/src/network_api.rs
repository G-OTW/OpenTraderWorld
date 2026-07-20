//! Network exposure settings: how the app binds to the host (localhost-only, LAN,
//! LAN with HTTPS, or a public domain with HTTPS).
//!
//! The actual port/binding is owned by Docker Compose, which can only change it by
//! recreating the container — core can't rebind a live socket, and (by design, like the
//! other host-level actions in `settings_api`) has no Docker access. So the flow is:
//!   1. core validates + persists the choice in `app_settings`,
//!   2. core renders a *secret-free* `network.env` (bind/ports/domain) and a `dns.env`
//!      (the ACME DNS line for Caddy — holds the DNS provider token in `lan_https` mode),
//!   3. the frontend tells the operator to run `docker compose up -d` to apply.
//!
//! `lan_https` gets a publicly trusted certificate for a LAN-only host: ownership of the
//! domain is proven with a DNS TXT record (ACME DNS-01) instead of an inbound challenge,
//! so nothing is exposed to the internet. The domain's A record simply points at the
//! host's private LAN IP (managed automatically for DuckDNS, manually for Cloudflare).
//!
//! Security: core writes TWO fixed files (never the real `.env`, which holds DB/crypto
//! secrets and is not mounted). Inputs are validated to fixed shapes before they ever
//! reach a file, so nothing user-controlled is interpolated loosely. The DNS token is
//! sealed (AEAD) in `app_settings` and never returned by the API.

use axum::{extract::State, routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/settings/network", get(get_network).post(set_network))
}

// Setting keys.
const K_MODE: &str = "net_mode"; // local | lan | lan_https | web
const K_PORT: &str = "net_port";
const K_DOMAIN: &str = "net_domain";
const K_DNS_PROVIDER: &str = "net_dns_provider"; // duckdns | cloudflare
const K_DNS_TOKEN: &str = "net_dns_token"; // sealed: hex(nonce):hex(ciphertext)
const K_LAN_IP: &str = "net_lan_ip";

// Secure default: a fresh install is reachable only from the host machine.
const DEFAULT_MODE: &str = "local";
const DEFAULT_PORT: &str = "5454";

async fn get_network(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    // No stored mode (fresh DB) but files configured by setup.sh → reflect the files,
    // so the UI doesn't show "local" over a live HTTPS setup (saving that would clobber it).
    let stored_mode = otw_store::settings::get(&state.pool, K_MODE).await?;
    if stored_mode.is_none() {
        if let Some(v) = network_from_files() {
            return Ok(Json(v));
        }
    }
    let mode = stored_mode.unwrap_or_else(|| DEFAULT_MODE.to_string());
    let port = otw_store::settings::get_or(&state.pool, K_PORT, DEFAULT_PORT).await?;
    let domain = otw_store::settings::get_or(&state.pool, K_DOMAIN, "").await?;
    let provider = otw_store::settings::get_or(&state.pool, K_DNS_PROVIDER, "duckdns").await?;
    let lan_ip = otw_store::settings::get_or(&state.pool, K_LAN_IP, "").await?;
    let token_set = otw_store::settings::get(&state.pool, K_DNS_TOKEN).await?.is_some()
        || dns_env_token().is_some();
    Ok(Json(json!({
        "mode": mode,
        "port": port,
        "domain": domain,
        "dns_provider": provider,
        "lan_ip": lan_ip,
        "dns_token_set": token_set,
    })))
}

#[derive(Deserialize)]
struct NetworkInput {
    /// `local` (127.0.0.1), `lan` (0.0.0.0, HTTP), `lan_https` (0.0.0.0, HTTPS via
    /// ACME DNS-01 — LAN stays private), or `web` (0.0.0.0, HTTPS via public domain).
    mode: String,
    port: u32,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    dns_provider: String,
    /// Empty = keep the previously saved token.
    #[serde(default)]
    dns_token: String,
    /// LAN IPv4 the domain should resolve to (DuckDNS record is updated automatically).
    #[serde(default)]
    lan_ip: String,
}

async fn set_network(
    State(state): State<AppState>,
    Json(input): Json<NetworkInput>,
) -> Result<Json<Value>, ApiError> {
    let mode = input.mode.trim();
    if !matches!(mode, "local" | "lan" | "lan_https" | "web") {
        return Err(ApiError::bad_request("mode must be local, lan, lan_https, or web"));
    }
    if input.port < 1 || input.port > 65535 {
        return Err(ApiError::bad_request("port must be between 1 and 65535"));
    }
    let domain = input.domain.trim();
    if matches!(mode, "web" | "lan_https") {
        if domain.is_empty() {
            return Err(ApiError::bad_request("this mode requires a domain"));
        }
        if !is_valid_hostname(domain) {
            return Err(ApiError::bad_request("invalid domain"));
        }
    }

    // lan_https extras: provider, token (possibly carried over), LAN IP.
    let mut acme_line = String::new();
    if mode == "lan_https" {
        let provider = input.dns_provider.trim();
        if !matches!(provider, "duckdns" | "cloudflare") {
            return Err(ApiError::bad_request("dns_provider must be duckdns or cloudflare"));
        }
        let token = match input.dns_token.trim() {
            "" => stored_token(&state).await?.ok_or_else(|| {
                ApiError::bad_request("a DNS provider token is required")
            })?,
            t => t.to_string(),
        };
        if !is_valid_token(&token) {
            return Err(ApiError::bad_request(
                "invalid token (letters, digits, - and _ only)",
            ));
        }

        let lan_ip = input.lan_ip.trim();
        if provider == "duckdns" {
            let sub = domain
                .strip_suffix(".duckdns.org")
                .filter(|s| !s.is_empty() && !s.contains('.'))
                .ok_or_else(|| {
                    ApiError::bad_request("DuckDNS domain must look like yourname.duckdns.org")
                })?;
            if lan_ip.parse::<std::net::Ipv4Addr>().is_err() {
                return Err(ApiError::bad_request("a valid LAN IPv4 address is required"));
            }
            // Point the record at the LAN IP before persisting anything, so a bad
            // token/subdomain fails the save loudly instead of breaking silently later.
            update_duckdns(&state.http, sub, &token, lan_ip).await?;
        }

        otw_store::settings::set(&state.pool, K_DNS_PROVIDER, provider).await?;
        otw_store::settings::set(&state.pool, K_LAN_IP, lan_ip).await?;
        let (nonce, ct) = state
            .cipher
            .seal(&token)
            .map_err(|_| ApiError::internal("could not seal DNS token"))?;
        let sealed = format!("{}:{}", hex_encode(&nonce), hex_encode(&ct));
        otw_store::settings::set(&state.pool, K_DNS_TOKEN, &sealed).await?;

        acme_line = format!("acme_dns {provider} {token}");
    }

    otw_store::settings::set(&state.pool, K_MODE, mode).await?;
    otw_store::settings::set(&state.pool, K_PORT, &input.port.to_string()).await?;
    otw_store::settings::set(&state.pool, K_DOMAIN, domain).await?;

    write_network_env(mode, input.port, domain)?;
    write_dns_env(&acme_line)?;

    tracing::warn!("network exposure changed to {mode} (port {}); restart required", input.port);
    Ok(Json(json!({
        "ok": true,
        "mode": mode,
        "port": input.port,
        "domain": domain,
        "restart_required": true,
    })))
}

/// Update the DuckDNS record for `sub` to `ip`. DuckDNS answers `OK` or `KO` (no detail).
async fn update_duckdns(
    http: &reqwest::Client,
    sub: &str,
    token: &str,
    ip: &str,
) -> Result<(), ApiError> {
    let url = format!("https://www.duckdns.org/update?domains={sub}&token={token}&ip={ip}");
    let body = http
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| {
            tracing::error!("duckdns update failed: {e}");
            ApiError::bad_request("could not reach DuckDNS to update the record")
        })?
        .text()
        .await
        .unwrap_or_default();
    if body.trim() != "OK" {
        return Err(ApiError::bad_request(
            "DuckDNS rejected the update — check the subdomain and token",
        ));
    }
    Ok(())
}

/// Previously saved token: sealed in `app_settings`, or (setup.sh installs that never
/// saved from the UI) plaintext in the dns.env file.
async fn stored_token(state: &AppState) -> Result<Option<String>, ApiError> {
    if let Some(sealed) = otw_store::settings::get(&state.pool, K_DNS_TOKEN).await? {
        let (n, c) = sealed
            .split_once(':')
            .and_then(|(n, c)| Some((hex_decode(n)?, hex_decode(c)?)))
            .ok_or_else(|| ApiError::internal("stored DNS token is malformed"))?;
        let token = state
            .cipher
            .open(&n, &c)
            .map_err(|_| ApiError::internal("could not unseal DNS token"))?;
        return Ok(Some(token));
    }
    Ok(dns_env_token())
}

/// Hostname check: labels of `[A-Za-z0-9-]`, dot-separated, no leading/trailing hyphen.
/// Deliberately strict so nothing shell- or newline-special can reach `network.env`.
fn is_valid_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// DNS provider tokens (DuckDNS UUIDs, Cloudflare API tokens) are URL-safe already;
/// enforcing that shape keeps the rendered Caddyfile line injection-proof.
fn is_valid_token(token: &str) -> bool {
    (8..=200).contains(&token.len())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Render the secret-free env file compose reads. Path is fixed and mounted read-write into
/// the container; the real `.env` (secrets) is never mounted here.
fn write_network_env(mode: &str, port: u32, domain: &str) -> Result<(), ApiError> {
    // Derive the compose knobs from the mode. All values are validated above, so this is a
    // fixed shape — no loose interpolation of free-form user text. Both HTTP and HTTPS host
    // ports are always emitted (compose publishes two port lines); in plain-HTTP modes the
    // HTTPS line is bound to loopback on a throwaway port so it never conflicts or exposes.
    // `bind` controls the HTTP port's interface; `https_bind` is loopback unless we're
    // actually serving TLS (lan_https/web). `domain_val` is Caddy's *site address*: `:80`
    // = catch-all plain HTTP (no TLS, no cert warnings, and it matches any Host — required
    // for LAN access by IP); a hostname triggers Caddy's automatic HTTPS (cert via DNS-01
    // when dns.env sets an acme_dns line, HTTP-01 otherwise). HTTPS modes pin 80/443 so
    // the automatic HTTP→HTTPS redirect lands on the right port.
    let (bind, https_bind, http_port, https_port, domain_val) = match mode {
        // Localhost only: loopback, HTTP on the chosen port.
        "local" => ("127.0.0.1", "127.0.0.1", port, 8443u32, ":80".to_string()),
        // LAN: all interfaces for HTTP, HTTPS stays loopback-only (unused).
        "lan" => ("0.0.0.0", "127.0.0.1", port, 8443u32, ":80".to_string()),
        // LAN + HTTPS / Web: all interfaces, HTTP→HTTPS redirect on 80, TLS on 443.
        "lan_https" | "web" => ("0.0.0.0", "0.0.0.0", 80, 443, domain.to_string()),
        _ => unreachable!("mode validated above"),
    };

    let body = format!(
        "# Generated by OpenTraderWorld — network exposure. Secret-free; safe to read.\n\
         # Edit via Settings → Network, then run: docker compose up -d\n\
         OTW_BIND={bind}\n\
         OTW_HTTPS_BIND={https_bind}\n\
         OTW_HTTP_PORT={http_port}\n\
         OTW_HTTPS_PORT={https_port}\n\
         OTW_DOMAIN={domain_val}\n"
    );

    write_env_file(&network_env_path(), &body)
}

/// Render the ACME DNS line compose passes to Caddy. Empty in every mode but `lan_https`.
/// Unlike network.env this file DOES hold a secret (the DNS provider token), which is why
/// it is a separate file — network.env stays safe to read/commit.
fn write_dns_env(acme_line: &str) -> Result<(), ApiError> {
    let body = format!(
        "# Generated by OpenTraderWorld — ACME DNS challenge for Caddy (LAN + HTTPS mode).\n\
         # Contains your DNS provider token when set: do not commit or share.\n\
         # Edit via Settings → Network, then run: docker compose up -d\n\
         OTW_ACME_GLOBAL={acme_line}\n"
    );
    write_env_file(&dns_env_path(), &body)
}

fn network_env_path() -> String {
    std::env::var("OTW_NETWORK_ENV").unwrap_or_else(|_| "/data/network.env".to_string())
}

fn dns_env_path() -> String {
    std::env::var("OTW_DNS_ENV").unwrap_or_else(|_| "/data/dns.env".to_string())
}

fn write_env_file(path: &str, body: &str) -> Result<(), ApiError> {
    std::fs::write(path, body).map_err(|e| {
        tracing::error!("writing {path}: {e}");
        ApiError::internal("could not write network config")
    })?;
    Ok(())
}

/// Best-effort read of the compose files for installs configured by setup.sh before any
/// UI save (no `net_mode` in the DB). Returns the same shape as `get_network`.
/// The effective network mode, resolved like `get_network`: stored setting, else the mode
/// the deploy files reflect (fresh DB over a setup.sh-configured install), else the secure
/// default. Used by session-cookie hardening (HTTPS modes set the Secure flag).
pub async fn effective_mode(pool: &sqlx::PgPool) -> String {
    if let Ok(Some(m)) = otw_store::settings::get(pool, K_MODE).await {
        return m;
    }
    network_from_files()
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_MODE.to_string())
}

fn network_from_files() -> Option<Value> {
    let net = std::fs::read_to_string(network_env_path()).ok()?;
    let get = |key: &str, from: &str| -> Option<String> {
        from.lines()
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('=').map(str::to_string))
    };
    let bind = get("OTW_BIND", &net)?;
    let domain = get("OTW_DOMAIN", &net)?;
    let http_port = get("OTW_HTTP_PORT", &net)?;

    let acme = std::fs::read_to_string(dns_env_path())
        .ok()
        .and_then(|s| get("OTW_ACME_GLOBAL", &s))
        .unwrap_or_default();
    // `acme_dns <provider> <token>`
    let mut acme_parts = acme.split_whitespace().skip(1);
    let provider = acme_parts.next().unwrap_or("duckdns").to_string();
    let token_set = acme_parts.next().is_some();

    let (mode, domain, port) = if domain == ":80" || domain.is_empty() {
        let m = if bind == "127.0.0.1" { "local" } else { "lan" };
        (m, String::new(), http_port)
    } else if token_set {
        ("lan_https", domain, "443".to_string())
    } else {
        ("web", domain, "443".to_string())
    };
    Some(json!({
        "mode": mode,
        "port": port,
        "domain": domain,
        "dns_provider": provider,
        "lan_ip": "",
        "dns_token_set": token_set,
    }))
}

/// Token from the dns.env file (setup.sh-written), if any.
fn dns_env_token() -> Option<String> {
    let s = std::fs::read_to_string(dns_env_path()).ok()?;
    let line = s
        .lines()
        .find_map(|l| l.trim().strip_prefix("OTW_ACME_GLOBAL="))?;
    line.split_whitespace().nth(2).map(str::to_string)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}
