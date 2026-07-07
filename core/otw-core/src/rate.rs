//! Process-wide API rate tracker — observe-and-alert only, never blocks a request.
//!
//! Outbound calls to external providers (market data, FX, feeds, quotes) route through here
//! so the Settings "API Rate" dashboard can show per-provider volume "since the beginning of
//! the day" and flag over-limit responses. There is deliberately **no throttling**: we record
//! what happened and, on a rate-limit hit, emit a WARN log and a dashboard event. Nothing here
//! can stop or delay a caller.
//!
//! Two entry points:
//!   • [`send`] wraps a `reqwest::RequestBuilder`: it fires the request, classifies the HTTP
//!     result (429 → limited, other >=400/network → error, else ok), records it, and hands the
//!     `Response` back untouched — a 429 is *not* turned into an error here, callers decide.
//!   • [`note_limited`] records a body-level "too many requests" note for providers that return
//!     HTTP 200 with an error message (Alpha Vantage's `Note`, Massive's non-OK envelope).
//!
//! Recording is fire-and-forget: each event is a detached task, so a slow/failed DB write never
//! stalls the outbound call path.

use otw_store::api_rate::{self, Outcome};

/// Extract a bare host (no scheme/port/path) from a URL for grouping.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Send a request through the tracker. Records the outcome, then returns the raw
/// `reqwest` result unchanged — a 429 comes back as a normal `Response` (status 429), so the
/// caller's existing error handling / retry logic is untouched. Nothing is blocked.
pub async fn send(
    provider: &str,
    builder: reqwest::RequestBuilder,
) -> reqwest::Result<reqwest::Response> {
    // Peek the URL for host grouping before consuming the builder.
    let host = builder
        .try_clone()
        .and_then(|b| b.build().ok())
        .map(|req| host_of(req.url().as_str()))
        .unwrap_or_default();

    let result = builder.send().await;
    match &result {
        Ok(resp) => {
            let status = resp.status();
            let outcome = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                Outcome::Limited
            } else if status.is_client_error() || status.is_server_error() {
                Outcome::Error
            } else {
                Outcome::Ok
            };
            let detail = if outcome == Outcome::Limited {
                // Retry-After, when present, is the most useful thing to surface.
                resp.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| format!("retry-after: {s}"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            api_rate::record(provider, &host, outcome, Some(status.as_u16() as i32), &detail);
        }
        Err(e) => {
            // Network/timeout/connect failure — count as an error, not a limit.
            api_rate::record(provider, &host, Outcome::Error, e.status().map(|s| s.as_u16() as i32), "");
        }
    }
    result
}

/// Record a body-level "too many requests" note for providers that return HTTP 200 with an
/// error message in the body (Alpha Vantage `Note`/`Information`, Massive non-OK envelope).
/// `url` is only used to derive the host; pass the request URL.
pub fn note_limited(provider: &str, url: &str, detail: &str) {
    api_rate::record(provider, &host_of(url), Outcome::Limited, None, detail);
}

// ── Known published limits (for the dashboard's "limit" column) ─────────────────────────
//
// Providers with a documented cap get a short human string; the rest show "no published
// limit". histdata providers reuse their `Capability.rate_limit` note verbatim; a few
// non-histdata providers (FX, feeds, crypto quotes) are listed here.

/// Human-readable published rate limit for `provider`, or `None` if none is documented.
pub fn known_limit(provider: &str) -> Option<&'static str> {
    // histdata connectors carry their own note.
    if let Some(cap) = crate::histdata::capabilities()
        .into_iter()
        .find(|c| c.provider == provider)
    {
        return Some(cap.rate_limit);
    }
    match provider {
        "frankfurter" => Some("Free, keyless. No hard published cap; be gentle — one pull/day is plenty."),
        "er-api" => Some("open.er-api.com free tier: ~1 request/day per base is enough; used only as the FX backup."),
        "binance" => Some("Public REST: 1200 request-weight/minute per IP. Quote lookups are 1 weight each."),
        "kraken" => Some("Public REST: ~1 request/second per IP (tier-based counter). Bursts get 429/EAPIRateLimit."),
        "coinbase" => Some("Public REST: ~10 requests/second per IP."),
        "coingecko" => Some("Free public API: ~10–30 calls/minute per IP (demo tier). Bursts return 429."),
        "dataroma" => Some("Unofficial scrape — no published limit. Polled on a slow schedule; don't hammer it."),
        "findb" => Some("GitHub release asset download — GitHub's standard unauthenticated limits apply."),
        _ => None,
    }
}
