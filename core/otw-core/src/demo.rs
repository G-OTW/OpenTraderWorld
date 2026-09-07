//! Demo mode — public shared sandbox posture.
//!
//! Activated by `OTW_DEMO=1`. Never enabled implicitly: a normal install is untouched.
//! Three pieces:
//!   1. A **default-deny route gate**: every request must match the allowlist below or it
//!      is rejected with 403 `demo_disabled`. New modules/routes are therefore blocked in
//!      demo until someone adds them here deliberately (same philosophy as the MCP catalog).
//!   2. A small **per-IP rate limiter** (the demo runs behind Caddy/Cloudflare, which set
//!      CF-Connecting-IP / X-Forwarded-For; direct hits fall back to a single shared
//!      bucket). The resolved IP is attached as [`ClientIp`] so the expensive endpoints
//!      can key their own [`WindowQuota`]s off it — per-visitor *and* globally.
//!   3. A public **status endpoint** (`GET /api/demo`) the frontend uses to show the
//!      sandbox banner and the next-reset countdown (resets are quarter-hour aligned).
//!   4. A boot-time **free-model resolver** ([`resolve_free_model`]) that repins the seeded
//!      agent onto a model the shared key can actually run today.

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use sqlx::PgPool;

use otw_store::agent as agent_store;
use otw_store::crypto::SecretCipher;

/// True when this process runs as the public demo sandbox.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("OTW_DEMO").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
    })
}

// ── Route policy ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Rule {
    /// Any method.
    Full,
    /// GET/HEAD only.
    ReadOnly,
    /// Nothing.
    Deny,
}

/// Ordered, first-match-wins. A prefix matches on a path-segment boundary
/// (`/api/mcp` matches `/api/mcp` and `/api/mcp/x`, never `/api/mcpx`).
/// Anything that matches no entry is denied — that is the point.
const RULES: &[(&str, Rule)] = &[
    // Infrastructure / auth handshake.
    ("/health", Rule::ReadOnly),
    ("/api/health", Rule::ReadOnly),
    ("/api/demo", Rule::ReadOnly),
    ("/api/login", Rule::Full),
    ("/api/setup/status", Rule::ReadOnly),
    ("/api/setup", Rule::Deny),
    // Inbound webhooks: public token route — a spam relay in a public sandbox.
    ("/api/hooks", Rule::Deny),
    // MCP is internal-only in demo: the agent reaches the gateway in-process through its
    // seeded read-only token (resolved by id — the plaintext was discarded at seed time),
    // while the external HTTP endpoint and all token/settings management stay closed.
    // The UI can still read the token list to show what the agent is allowed to do.
    ("/api/mcp/settings", Rule::ReadOnly),
    ("/api/mcp/tokens", Rule::ReadOnly),
    ("/api/mcp", Rule::Deny),
    // Agent: chat/conversations are the showcase. Provider and external MCP-server
    // management stay read-only (keys, SSRF); run-time limits live in agent_api.
    ("/api/agent/providers", Rule::ReadOnly),
    ("/api/agent/mcp-servers", Rule::ReadOnly),
    // Memories and skills are read-only: they are single-user, globally shared state, so
    // one visitor's writes would leak into every other visitor's agent context (and be
    // wiped mid-conversation by the 15-minute reset anyway). Browsing the seeded set still
    // shows what the feature does. The agent's own tools are withheld to match.
    ("/api/agent/memories", Rule::ReadOnly),
    ("/api/agent/skills", Rule::ReadOnly),
    ("/api/agent", Rule::Full),
    // Automator: the graph, its saved revisions and its run history are worth reading, and
    // that is all. A workflow *runs* blocks that call an arbitrary URL, dispatch a
    // notification and spend the shared LLM key, and a schedule does it again every night
    // long after the visitor left — so running one, testing one, or storing one to run
    // later all stay closed. Read-only keeps the module explorable without turning the
    // sandbox into an open outbound relay.
    ("/api/automator", Rule::ReadOnly),
    // Paper trading schedules a recurring simulation and pushes it to a notification
    // channel: a shared sandbox has no business creating either.
    ("/api/backtest/paper", Rule::Deny),
    // Backtesting on seeded datasets (concurrency + window caps live in backtest_api).
    ("/api/backtest", Rule::Full),
    ("/api/calendar", Rule::Full),
    // Community docs: browsing is fine; submit/refresh/sync relay outbound — blocked.
    ("/api/community-docs/submit", Rule::Deny),
    ("/api/community-docs/refresh", Rule::Deny),
    ("/api/community-docs/sync", Rule::Deny),
    ("/api/community-docs", Rule::Full),
    // Data broker: provider capabilities and the connector list are browsable; creating a
    // connector or touching its credentials is not.
    ("/api/connectors", Rule::ReadOnly),
    ("/api/dashboard", Rule::Full),
    ("/api/databases", Rule::Full),
    ("/api/documents", Rule::Full),
    // News: readable + live stream; feed management would let visitors point the
    // scheduler at arbitrary URLs (SSRF) — read-only.
    ("/api/feeds", Rule::ReadOnly),
    ("/api/feed-dashboards", Rule::ReadOnly),
    ("/api/feed-items", Rule::ReadOnly),
    ("/api/feed-sources", Rule::ReadOnly),
    // Files: serving embedded images is needed; uploads are not (classic abuse vector).
    ("/api/files", Rule::ReadOnly),
    // FinanceDatabase: search the seeded install; the bulk import is heavy — blocked, and
    // with it the update check, which would only offer an update the sandbox cannot apply.
    ("/api/findb/install", Rule::Deny),
    ("/api/findb/update-check", Rule::Deny),
    ("/api/findb", Rule::Full),
    ("/api/goals", Rule::Full),
    // Historical data: seeded datasets are browsable; exports, imports, previews and the
    // download queue are not ("no dataset downloads" rule).
    ("/api/histdata/datasets", Rule::ReadOnly), // GET bars/list; append/export handled below
    // Symbol lookup is the chart's front door: without it the sandbox can only show the
    // four seeded instruments, and the point of the module is that you type a ticker and it
    // appears. It reaches the provider, so it draws on the same budget as the series reads.
    ("/api/histdata/symbols", Rule::ReadOnly),
    ("/api/histdata", Rule::Deny),
    // Charting is the showcase, so the workspace, its lists and the per-instrument layouts
    // are writable: they are the visitor's own screen, and the 15-minute reset takes them
    // back. Two carve-outs:
    //   * an alert is a *background* job — it keeps re-reading the provider on its own
    //     cadence long after the visitor closed the tab, and the seeded rows come back at
    //     every reset — so alerts are browsable, not creatable;
    //   * the series and stream routes reach a provider, and are quota'd in `histdata_api`
    //     (per-IP + global for the reads, a concurrency cap for the open SSE streams).
    ("/api/histviz/alerts", Rule::ReadOnly),
    ("/api/histviz", Rule::Full),
    // The journal's market-data sync queues histdata downloads, and the demo refuses
    // those wholesale; reading the coverage and measuring against seeded bars stay open.
    ("/api/journal/market/sync", Rule::Deny),
    ("/api/journal", Rule::Full),
    // Mailbox: the seeded mail is readable and the store is playable with. Connecting a
    // mailbox, polling one, downloading attachments or unsubscribing all reach outside
    // the box (or need the vault) — blocked, so the demo shows the module, not the wires.
    ("/api/mailbox/accounts", Rule::ReadOnly),
    ("/api/mailbox/poll", Rule::Deny),
    ("/api/mailbox/oauth", Rule::Deny),
    ("/api/mailbox/attachments", Rule::Deny),
    ("/api/mailbox/settings", Rule::ReadOnly),
    ("/api/mailbox", Rule::Full),
    ("/api/mindset", Rule::Full),
    // Managers' portfolios: cached data is browsable; manual scrape trigger is not.
    ("/api/mportfolios/refresh", Rule::Deny),
    ("/api/mportfolios", Rule::ReadOnly),
    // External connectors (mail/Telegram/Slack/Discord…) are out, wholesale.
    ("/api/notif-channels", Rule::Deny),
    ("/api/notifications", Rule::Full),
    ("/api/portfolios", Rule::Full),
    ("/api/prompts", Rule::Full),
    ("/api/quant", Rule::Full),
    ("/api/rate", Rule::ReadOnly),
    ("/api/reminders", Rule::Full),
    // Resources: the library is playable with, but the thumbnail preview fetches a URL a
    // visitor supplies — an open outbound relay in a public sandbox.
    ("/api/resources/preview", Rule::Deny),
    ("/api/resources", Rule::Full),
    ("/api/search", Rule::Full),
    // Settings: readable where harmless, but NO settings modification at all — the UI
    // stays clickable and every attempt gets the explanatory 403 the frontend toasts.
    ("/api/settings/me", Rule::ReadOnly),
    ("/api/settings/version", Rule::ReadOnly),
    ("/api/settings/defaults", Rule::ReadOnly),
    // Logout is a dead end here: auto-login means there is no session to end (the next
    // request just re-resolves the seeded admin), but the frontend still redirects to
    // /login — stranding the visitor on a sign-in form whose credentials are not public.
    // Deny, so the existing `demo_disabled` toast explains it instead.
    ("/api/settings/logout", Rule::Deny),
    ("/api/settings/modules/install", Rule::Deny),
    ("/api/settings/modules/detach", Rule::Deny),
    ("/api/settings/modules", Rule::ReadOnly),
    ("/api/settings", Rule::Deny),
    ("/api/subscriptions", Rule::Full),
    ("/api/taxcalc", Rule::Full),
    ("/api/time", Rule::Full),
    ("/api/todos", Rule::Full),
    ("/api/trader", Rule::Full),
    // Secrets vault: never in a public sandbox.
    ("/api/vault", Rule::Deny),
    // Resolving an embed fetches a URL the visitor pasted: the same outbound relay the
    // resource preview is denied for. Written down rather than left to the default deny,
    // so it stays denied on purpose.
    ("/api/video-embed", Rule::Deny),
    ("/api/watchlists", Rule::Full),
    ("/api/wealth", Rule::Full),
    // Webhook management mints secrets.
    ("/api/webhooks", Rule::Deny),
];

fn matches_prefix(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn allowed(method: &Method, path: &str) -> bool {
    // Dataset export is a GET under an otherwise read-only prefix — deny it explicitly.
    if matches_prefix(path, "/api/histdata/datasets") && path.ends_with("/export") {
        return false;
    }
    // One-click unsubscribe POSTs to a sender's URL: an outbound call, under an
    // otherwise writable prefix (the message id sits mid-path, so no prefix rule fits).
    if matches_prefix(path, "/api/mailbox/messages") && path.ends_with("/unsubscribe") {
        return false;
    }
    for (prefix, rule) in RULES {
        if matches_prefix(path, prefix) {
            return match rule {
                Rule::Full => true,
                Rule::ReadOnly => matches!(*method, Method::GET | Method::HEAD),
                Rule::Deny => false,
            };
        }
    }
    false
}

// ── Per-IP rate limiter ──────────────────────────────────────────────────────

/// Requests allowed per client IP per minute. Generous — the SPA is chatty — but a
/// scripted hammer hits it fast. Cloudflare's WAF is the coarse outer layer.
const IP_LIMIT: u32 = 300;
const IP_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
/// Hard cap on tracked IPs so the map cannot grow unbounded.
const IP_MAP_MAX: usize = 20_000;

type IpMap = std::collections::HashMap<String, (std::time::Instant, u32)>;
static IP_HITS: std::sync::Mutex<Option<IpMap>> = std::sync::Mutex::new(None);

fn ip_throttled(ip: &str) -> bool {
    let mut guard = IP_HITS.lock().unwrap();
    let map = guard.get_or_insert_with(IpMap::new);
    if map.len() >= IP_MAP_MAX {
        map.retain(|_, (start, _)| start.elapsed() < IP_WINDOW);
        if map.len() >= IP_MAP_MAX {
            return true; // saturated: fail closed rather than grow
        }
    }
    let now = std::time::Instant::now();
    let entry = map.entry(ip.to_string()).or_insert((now, 0));
    if entry.0.elapsed() >= IP_WINDOW {
        *entry = (now, 0);
    }
    entry.1 += 1;
    entry.1 > IP_LIMIT
}

fn client_ip(req: &Request) -> String {
    // Cloudflare's CF-Connecting-IP is the trustworthy one when the demo runs behind it:
    // unlike X-Forwarded-For it is rewritten (not appended to) at the edge, so a client
    // cannot forge a leading hop. Fall back to XFF for a bare Caddy deployment.
    req.headers()
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            req.headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "direct".to_string())
}

/// The client IP resolved by [`gate`], attached to every gated request so quota-bearing
/// handlers can key off it without re-parsing headers (and without being able to see a
/// header the gate did not vet).
#[derive(Clone)]
pub struct ClientIp(pub String);

/// Handlers read it as `Option<Extension<ClientIp>>`: requests that never passed the gate
/// — in-process MCP tool dispatch, and every non-demo deployment — see `None` and collapse
/// to one shared bucket, which is the conservative choice (MCP runs are already bounded by
/// their own per-token limits).

/// A tiny reusable window counter for demo-only quotas (agent runs, backtests…).
///
/// Two budgets, both enforced: a **per-IP** one so a single visitor cannot consume the
/// sandbox for everyone, and a **global** ceiling on top so the shared spend (the free-tier
/// LLM key, the 2-core host) stays bounded no matter how many distinct IPs show up.
/// Per-IP alone would not cap spend; global alone lets one scripted visitor lock out the
/// rest, which is the denial-of-service the per-IP half exists to close.
pub struct WindowQuota {
    /// Global budget: (window start, hits).
    global: std::sync::Mutex<(Option<std::time::Instant>, u32)>,
    /// Per-IP budgets, pruned lazily once the map grows past [`QUOTA_MAP_MAX`].
    per_ip: std::sync::Mutex<Option<IpMap>>,
    limit: u32,
    per_ip_limit: u32,
    window: std::time::Duration,
}

/// Hard cap on tracked IPs per quota, mirroring [`IP_MAP_MAX`].
const QUOTA_MAP_MAX: usize = 20_000;

impl WindowQuota {
    /// `limit` is the shared ceiling, `per_ip_limit` the slice any one client may take.
    pub const fn new(limit: u32, per_ip_limit: u32, window: std::time::Duration) -> Self {
        Self {
            global: std::sync::Mutex::new((None, 0)),
            per_ip: std::sync::Mutex::new(None),
            limit,
            per_ip_limit,
            window,
        }
    }

    /// Count one hit against `ip` and the global pool; false when either budget is spent.
    ///
    /// The per-IP budget is checked first and the global counter is only incremented once
    /// the client is under its own cap — otherwise a single hammering IP would still drain
    /// the shared pool through rejected calls.
    pub fn allow(&self, ip: &str) -> bool {
        {
            let mut guard = self.per_ip.lock().unwrap();
            let map = guard.get_or_insert_with(IpMap::new);
            if map.len() >= QUOTA_MAP_MAX {
                map.retain(|_, (start, _)| start.elapsed() < self.window);
                if map.len() >= QUOTA_MAP_MAX {
                    return false; // saturated: fail closed rather than grow
                }
            }
            let now = std::time::Instant::now();
            let entry = map.entry(ip.to_string()).or_insert((now, 0));
            if entry.0.elapsed() >= self.window {
                *entry = (now, 0);
            }
            if entry.1 >= self.per_ip_limit {
                return false;
            }
            entry.1 += 1;
        }

        let mut guard = self.global.lock().unwrap();
        let now = std::time::Instant::now();
        match guard.0 {
            Some(start) if now.duration_since(start) < self.window => {
                if guard.1 >= self.limit {
                    return false;
                }
                guard.1 += 1;
            }
            _ => *guard = (Some(now), 1),
        }
        true
    }
}

// ── Live-stream slots ────────────────────────────────────────────────────────

/// Live SSE streams the sandbox holds open at once.
///
/// A stream is one request that never ends, so neither the per-minute limiter nor a
/// [`WindowQuota`] bounds it: a scripted client would park thousands of them on the
/// two-core host while spending almost no request budget. What is capped here is the
/// demo's own sockets — the upstream cost is already bounded by the hub, which keeps one
/// provider connection per connector however many readers it has.
const LIVE_MAX: usize = 64;
static LIVE_OPEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Held for as long as a live stream is; releases its slot when the stream is dropped
/// (client gone, pane closed, request cancelled).
pub enum LiveSlot {
    /// Not a demo: nothing is counted and nothing is released.
    Unlimited,
    Counted,
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        if matches!(self, LiveSlot::Counted) {
            LIVE_OPEN.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Take a slot, or `None` when the sandbox is already holding [`LIVE_MAX`] streams.
pub fn live_slot() -> Option<LiveSlot> {
    use std::sync::atomic::Ordering;
    if !enabled() {
        return Some(LiveSlot::Unlimited);
    }
    // Compare-and-swap rather than fetch_add: an increment past the cap would have to be
    // undone, and two callers racing at the ceiling would both see themselves over it.
    let mut open = LIVE_OPEN.load(Ordering::Relaxed);
    loop {
        if open >= LIVE_MAX {
            return None;
        }
        match LIVE_OPEN.compare_exchange_weak(
            open,
            open + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(LiveSlot::Counted),
            Err(actual) => open = actual,
        }
    }
}

// ── Middleware + status ──────────────────────────────────────────────────────

/// Outermost demo layer: rate-limit, then enforce the route allowlist.
pub async fn gate(mut req: Request, next: Next) -> Response {
    // Owned: the extension insert below needs `req` mutably while this is still in use.
    let path = req.uri().path().to_string();
    let path = path.as_str();
    // Only API traffic is gated; Caddy serves the SPA itself.
    if path.starts_with("/api") || path == "/health" {
        let ip = client_ip(&req);
        // Hand the resolved IP down to quota-bearing handlers (agent runs, backtests).
        req.extensions_mut().insert(ClientIp(ip.clone()));
        if ip_throttled(&ip) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({ "error": "demo rate limit reached — slow down a little" })),
            )
                .into_response();
        }
        if !allowed(req.method(), path) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "demo_disabled", "message": "this action is disabled in the public demo" })),
            )
                .into_response();
        }
    }
    next.run(req).await
}

// ── Shared free-tier key ─────────────────────────────────────────────────────

/// The shared free-tier key injected by the host (`OTW_DEMO_LLM_KEY`).
///
/// The seeded provider row carries an EMPTY `api_key` on purpose — the seed is a public
/// artifact — so in demo mode the key lives only in the environment. Every path that
/// needs a key (run, model listing, boot-time repin) resolves the stored key first and
/// falls back here. Returns `None` outside demo mode or when the host set no key.
pub fn shared_llm_key() -> Option<String> {
    if !enabled() {
        return None;
    }
    std::env::var("OTW_DEMO_LLM_KEY").ok().filter(|k| !k.is_empty())
}

// ── Free-model resolver ──────────────────────────────────────────────────────
//
// OpenRouter's `:free` catalogue churns: a slug that is free today can lose its free
// tier next month, after which the seeded model 404s ("unavailable for free — use the
// paid slug") and the demo agent is silently dead. The seed can't be the source of
// truth, because the `otw_seed` template is built once and then replayed by every
// 15-minute reset for weeks.
//
// So the seeded slug is only a *preference*: at boot we ask the provider what is free
// RIGHT NOW and repin the provider/agent rows onto something that actually runs. A
// retired slug simply stops appearing in `/models`, which makes the live list a
// reliable oracle. Best-effort by design — the demo must boot even when OpenRouter is
// unreachable, so every failure path leaves the seeded value in place.

/// Ordered fallbacks, tried only when the seeded model is no longer free. Small,
/// fast, tool-capable models first: the sandbox showcases tool calls, and a 500B
/// reasoning model would make every demo turn crawl.
const FREE_MODEL_PREFS: &[&str] = &[
    "openai/gpt-oss-20b:free",
    "nvidia/nemotron-nano-9b-v2:free",
    "google/gemma-4-31b-it:free",
];

/// Repin the demo agent onto a currently-free model. Called once at boot, after
/// migrations. Never fails the boot: on any error the seeded slug stays as-is.
pub async fn resolve_free_model(pool: &PgPool, cipher: &SecretCipher) {
    if !enabled() {
        return;
    }
    if let Err(e) = repin(pool, cipher).await {
        // A dead demo agent is a papercut, not an outage — log and carry on.
        tracing::warn!("demo: could not resolve a free model ({e}); keeping the seeded one");
    }
}

async fn repin(pool: &PgPool, cipher: &SecretCipher) -> anyhow::Result<()> {
    let Some(prow) = agent_store::list_providers(pool)
        .await?
        .into_iter()
        .find(|p| p.kind == "openai_compat" && p.base_url.contains("openrouter.ai"))
    else {
        return Ok(()); // not the seeded OpenRouter demo provider — nothing to do
    };

    // Same key resolution as the run path: the seeded row carries no key, the shared
    // free-tier key comes from the host environment.
    let key = match agent_store::provider_key(pool, cipher, prow.id).await?.filter(|k| !k.is_empty()) {
        Some(k) => k,
        None => match shared_llm_key() {
            Some(k) => k,
            None => return Ok(()), // no key configured: nothing to validate against
        },
    };

    let provider = crate::agent::build_provider(&prow.kind, &prow.base_url, key)
        .ok_or_else(|| anyhow::anyhow!("unknown provider kind {}", prow.kind))?;
    let free: Vec<String> = provider
        .models()
        .await?
        .into_iter()
        .filter(|m| m.ends_with(":free"))
        .collect();
    if free.is_empty() {
        anyhow::bail!("provider listed no ':free' models");
    }

    // The seeded slug still works — the common case, so say nothing and change nothing.
    if free.iter().any(|m| m == &prow.default_model) {
        return Ok(());
    }

    // Prefer a curated fallback: those are hand-checked to support tool calls, which the
    // sandbox showcases. `models()` returns bare ids (no `supported_parameters`), so the
    // last resort — used only if every curated slug is retired at once — can't be vetted
    // for tools; a chat-only demo still beats a dead one, and the log names the pick.
    let pick = FREE_MODEL_PREFS
        .iter()
        .find(|c| free.iter().any(|m| m == *c))
        .map(|c| (*c).to_string())
        .or_else(|| free.first().cloned())
        .expect("free is non-empty");

    agent_store::repin_demo_model(pool, prow.id, &pick).await?;
    tracing::warn!(
        "demo: seeded model '{}' is no longer free — repinned to '{pick}'",
        prow.default_model
    );
    Ok(())
}

/// GET /api/demo — public. `next_reset_at` is the next quarter-hour boundary (UTC),
/// matching the host cron that restores the seed database.
pub async fn status() -> Json<serde_json::Value> {
    if !enabled() {
        return Json(json!({ "demo": false }));
    }
    let now = time::OffsetDateTime::now_utc();
    let secs_into_quarter = (now.minute() as i64 % 15) * 60 + now.second() as i64;
    let next = now + time::Duration::seconds(15 * 60 - secs_into_quarter);
    Json(json!({
        "demo": true,
        "next_reset_at": next
            .replace_nanosecond(0)
            .unwrap_or(next)
            .format(&time::format_description::well_known::Rfc3339)
            .ok(),
        "reset_minutes": 15,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const WINDOW: Duration = Duration::from_secs(600);

    #[test]
    fn chart_is_writable_but_its_background_jobs_are_not() {
        // The workspace is the visitor's own screen; an alert outlives the visit.
        assert!(allowed(&Method::POST, "/api/histviz/workspaces"));
        assert!(allowed(&Method::PUT, "/api/histviz/layout"));
        assert!(allowed(&Method::POST, "/api/histviz/series/batch"));
        assert!(allowed(&Method::GET, "/api/histviz/streams"));
        assert!(allowed(&Method::GET, "/api/histviz/alerts"));
        assert!(!allowed(&Method::POST, "/api/histviz/alerts"));
    }

    #[test]
    fn automator_is_readable_and_never_runnable() {
        assert!(allowed(&Method::GET, "/api/automator/workflows"));
        assert!(allowed(&Method::GET, "/api/automator/runs"));
        for path in [
            "/api/automator/workflows",
            "/api/automator/workflows/x/run",
            "/api/automator/workflows/x/test",
            "/api/automator/nodes/test",
            "/api/automator/schedules",
        ] {
            assert!(!allowed(&Method::POST, path), "{path} must not be runnable");
        }
    }

    #[test]
    fn outbound_relays_stay_shut() {
        assert!(!allowed(&Method::POST, "/api/video-embed"));
        assert!(!allowed(&Method::POST, "/api/resources/preview"));
        assert!(!allowed(&Method::POST, "/api/histdata/preview"));
    }

    #[test]
    fn live_slots_are_released_when_the_stream_ends() {
        // Outside demo mode nothing is counted, which is the case this asserts: a slot is
        // always available and dropping it changes no shared state.
        let a = live_slot().expect("no cap outside demo mode");
        let b = live_slot().expect("no cap outside demo mode");
        drop((a, b));
        assert_eq!(LIVE_OPEN.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn per_ip_budget_is_isolated() {
        // The point of the per-IP half: one visitor exhausting its slice must not
        // affect another visitor who has spent nothing.
        let q = WindowQuota::new(100, 2, WINDOW);
        assert!(q.allow("a"));
        assert!(q.allow("a"));
        assert!(!q.allow("a"), "third call from `a` exceeds the per-IP cap");
        assert!(q.allow("b"), "`b` must be unaffected by `a` draining its slice");
    }

    #[test]
    fn global_ceiling_still_caps_many_ips() {
        // The global half: distinct IPs must not be able to exceed the shared budget.
        let q = WindowQuota::new(3, 2, WINDOW);
        assert!(q.allow("a"));
        assert!(q.allow("b"));
        assert!(q.allow("c"));
        assert!(!q.allow("d"), "global ceiling reached despite `d` being fresh");
    }

    #[test]
    fn rejected_calls_do_not_drain_the_global_pool() {
        // An IP over its own cap must not consume shared budget through refusals,
        // otherwise a hammering client still starves everyone else.
        let q = WindowQuota::new(3, 1, WINDOW);
        assert!(q.allow("spammer"));
        for _ in 0..50 {
            assert!(!q.allow("spammer"));
        }
        assert!(q.allow("x"));
        assert!(q.allow("y"), "global pool should still have budget left");
    }
}
