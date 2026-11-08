//! Cross-cutting request-security primitives: client identity, attempt throttling,
//! same-origin enforcement and step-up re-authentication.
//!
//! Everything here exists because of one deployment shape: `web` mode, where the app answers
//! the open internet with a single account behind it. On localhost these are inert or
//! cheap; none of them changes what an authenticated user may do.
//!
//! The throttles are in-process on purpose. A single-binary self-hosted app has no shared
//! cache to put them in, and a counter that resets when the container restarts is still the
//! difference between thousands of guesses an hour and millions.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::{header, HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use axum_extra::extract::cookie::CookieJar;

use crate::ApiError;

/// Cap on distinct keys any one throttle tracks, so a spray from a botnet cannot grow the
/// map without bound. Reaching it clears the oldest half rather than refusing to record:
/// forgetting an attacker's counter is recoverable, running out of memory is not.
const MAX_KEYS: usize = 20_000;

// ── Who is asking ────────────────────────────────────────────────────────────

/// The caller's address as far as this process can tell.
///
/// `CF-Connecting-IP` first: behind Cloudflare it is rewritten at the edge, so a client
/// cannot forge a leading hop the way it can with `X-Forwarded-For`. XFF's first entry is
/// the fallback for a bare Caddy deployment, which is the documented topology here (Caddy
/// sets it and core is not otherwise reachable). A direct hit with neither header collapses
/// to one shared bucket, named so the logs say as much.
///
/// Never an authorization input. It keys rate limits and labels a session row; nothing is
/// granted because of it.
pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "direct".to_string())
}

/// The browser's self-description, trimmed to something a settings list can show.
pub fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

/// The stable identity of the caller's session: the SHA-256 of its cookie token, which is
/// also what the `sessions` row is keyed by. Used to hang a step-up grant off a session
/// without holding the session secret in memory.
///
/// Reads whichever cookie name the deployment set: in the HTTPS modes the session lives in
/// `__Host-otw_session`, and reading only the plain name there left every step-up refused.
pub fn session_key(jar: &CookieJar) -> Option<String> {
    crate::session_token(jar).map(|t| otw_store::mcp::hash_token(&t))
}

// ── Attempt counters ─────────────────────────────────────────────────────────

/// Failed attempts per key inside a rolling window.
///
/// One window per key, started by its first failure and reset once it lapses. Not a token
/// bucket: the thing being limited is *wrong guesses*, and a counter that resets on success
/// is what keeps a legitimate user who mistypes once from paying for it.
pub struct Attempts {
    window: Duration,
    buckets: Mutex<Option<HashMap<String, (Instant, u32)>>>,
}

impl Attempts {
    pub const fn new(window: Duration) -> Self {
        Self { window, buckets: Mutex::new(None) }
    }

    /// Failures recorded against this key in the live window.
    pub fn count(&self, key: &str) -> u32 {
        let guard = self.buckets.lock().unwrap();
        guard
            .as_ref()
            .and_then(|m| m.get(key))
            .filter(|(start, _)| start.elapsed() < self.window)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    /// Record one failure and return the new count.
    pub fn record(&self, key: &str) -> u32 {
        let mut guard = self.buckets.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() >= MAX_KEYS {
            // Drop everything whose window has lapsed; if that frees nothing, drop it all.
            let window = self.window;
            map.retain(|_, (start, _)| start.elapsed() < window);
            if map.len() >= MAX_KEYS {
                map.clear();
            }
        }
        let entry = map.entry(key.to_string()).or_insert((Instant::now(), 0));
        if entry.0.elapsed() >= self.window {
            *entry = (Instant::now(), 0);
        }
        entry.1 += 1;
        entry.1
    }

    /// Forget this key. Called on success: a correct credential proves the traffic was not
    /// an attack, and leaving the counter up would punish the next honest mistake.
    pub fn clear(&self, key: &str) {
        if let Some(map) = self.buckets.lock().unwrap().as_mut() {
            map.remove(key);
        }
    }
}

// ── Same-origin enforcement ──────────────────────────────────────────────────

/// Reject a state-changing request whose browser says it came from another site.
///
/// `SameSite=Lax` on the session cookie already stops the classic cross-site POST, and this
/// codebase has no state-changing `GET`. This is the second lock OWASP asks for (ASVS
/// V4.2.2): it holds even if a future route is added as a `GET`, if a browser relaxes Lax,
/// or if a sibling subdomain is compromised.
///
/// The rules, in order:
/// * Safe methods pass. They change nothing.
/// * `Sec-Fetch-Site` decides when present. Every current browser sends it and it cannot be
///   set by script, so it is the strongest signal available. `same-origin` and `none` (a
///   typed URL or a bookmark) pass; `same-site` and `cross-site` do not.
/// * Otherwise `Origin` must match `Host` when `Origin` is present.
/// * A request with neither header is not a browser: `curl`, an MCP client, the in-process
///   dispatch the Automator and the MCP gateway run through. Those carry no ambient cookie,
///   so they are not a CSRF vector, and refusing them would break the API for scripts.
pub async fn same_origin(req: Request, next: Next) -> Result<Response, ApiError> {
    if matches!(req.method(), &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return Ok(next.run(req).await);
    }
    let headers = req.headers();

    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return match site.trim() {
            "same-origin" | "none" => Ok(next.run(req).await),
            other => Err(ApiError::forbidden_code(
                "cross_origin",
                &format!("this request was made from another site ({other})"),
            )),
        };
    }

    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        None => Ok(next.run(req).await),
        Some(origin) => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            let origin_host = origin
                .split_once("://")
                .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
                .unwrap_or_default();
            if !host.is_empty() && origin_host.eq_ignore_ascii_case(host) {
                Ok(next.run(req).await)
            } else {
                Err(ApiError::forbidden_code(
                    "cross_origin",
                    "this request was made from another site",
                ))
            }
        }
    }
}

// ── Step-up re-authentication ────────────────────────────────────────────────

/// How long one password re-entry authorizes sensitive work. Long enough to finish what
/// was started (mint a token, change the network mode, export a bundle), short enough that
/// a session left open on a shared screen is not a standing grant.
pub const STEP_UP_TTL: Duration = Duration::from_secs(5 * 60);

/// Sessions that have re-entered their password recently.
///
/// Keyed by session token hash, so a grant dies with the session that earned it and cannot
/// be moved to another. In memory only: a restart asks again, which is the safe direction.
pub struct StepUp {
    grants: Mutex<Option<HashMap<String, Instant>>>,
    /// Single-use proofs, for the actions that ask for the password *every* time (turning
    /// trading on for a broker account). Kept apart from `grants` so a password typed a
    /// minute ago for something else never stands in for one of these.
    once: Mutex<Option<HashMap<String, Instant>>>,
}

/// How long a single-use proof waits to be spent: the time to answer the prompt and let the
/// refused request run again, not a window.
pub const STEP_UP_ONCE_TTL: Duration = Duration::from_secs(60);

impl StepUp {
    pub const fn new() -> Self {
        Self { grants: Mutex::new(None), once: Mutex::new(None) }
    }

    /// Record a single-use proof for this session: the next [`require_password_once`] spends it.
    pub fn grant_once(&self, session_key: &str) {
        let mut guard = self.once.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        map.retain(|_, at| at.elapsed() < STEP_UP_ONCE_TTL);
        if map.len() < MAX_KEYS {
            map.insert(session_key.to_string(), Instant::now());
        }
    }

    /// Spend this session's single-use proof. True when there was a live one.
    fn take_once(&self, session_key: &str) -> bool {
        self.once
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|m| m.remove(session_key))
            .is_some_and(|at| at.elapsed() < STEP_UP_ONCE_TTL)
    }

    /// Record that this session just proved its password (and its second factor, if any).
    pub fn grant(&self, session_key: &str) {
        let mut guard = self.grants.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        map.retain(|_, at| at.elapsed() < STEP_UP_TTL);
        if map.len() < MAX_KEYS {
            map.insert(session_key.to_string(), Instant::now());
        }
    }

    fn holds(&self, session_key: &str) -> bool {
        self.grants
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|m| m.get(session_key))
            .is_some_and(|at| at.elapsed() < STEP_UP_TTL)
    }

    /// Whether this session currently holds a grant. The read-only half of
    /// [`require_step_up`], for a caller that needs to know *before* it acts: a download is
    /// a plain browser navigation, so it cannot be refused and retried.
    pub fn is_valid(&self, session_key: &str) -> bool {
        self.holds(session_key)
    }

    /// Drop a session's grant. Called when the session ends or its password changes.
    pub fn revoke(&self, session_key: &str) {
        if let Some(map) = self.grants.lock().unwrap().as_mut() {
            map.remove(session_key);
        }
        if let Some(map) = self.once.lock().unwrap().as_mut() {
            map.remove(session_key);
        }
    }
}

/// The process-wide grant table.
pub static STEP_UP: StepUp = StepUp::new();

// ── Trading unlock ───────────────────────────────────────────────────────────

/// Sessions that have opened trading with an authenticator code, while the trading lock is
/// on.
///
/// Keyed by session token hash: a code typed in one browser opens trading on every account
/// with the trading grant, for that browser and nothing else. No expiry of its own, it lives
/// as long as the session, which is what "once per sign-in" means. In memory only, so a
/// restart closes trading again, which is the safe direction.
pub struct TradingUnlocks {
    open: Mutex<Option<std::collections::HashSet<String>>>,
}

impl TradingUnlocks {
    pub const fn new() -> Self {
        Self { open: Mutex::new(None) }
    }

    pub fn grant(&self, session_key: &str) {
        let mut guard = self.open.lock().unwrap();
        let set = guard.get_or_insert_with(Default::default);
        if set.len() >= MAX_KEYS {
            // Full means something is spraying sessions; dropping every unlock only costs
            // the owner one more code.
            set.clear();
        }
        set.insert(session_key.to_string());
    }

    pub fn holds(&self, session_key: &str) -> bool {
        self.open.lock().unwrap().as_ref().is_some_and(|s| s.contains(session_key))
    }

    /// Close trading for a session. Called when the session ends.
    pub fn revoke_session(&self, session_key: &str) {
        if let Some(set) = self.open.lock().unwrap().as_mut() {
            set.remove(session_key);
        }
    }

    /// Close trading in every session: the lock was just turned on or reset.
    pub fn clear(&self) {
        if let Some(set) = self.open.lock().unwrap().as_mut() {
            set.clear();
        }
    }
}

pub static TRADING_UNLOCK: TradingUnlocks = TradingUnlocks::new();

/// Wrong codes per session inside [`TRADING_UNLOCK_FAILS`]'s window before the session is
/// refused outright. Per session, not per account or global: a stolen cookie burns only its
/// own tries and can never lock the owner's own browser out.
pub const TRADING_UNLOCK_MAX_FAILS: u32 = 5;

pub static TRADING_UNLOCK_FAILS: Attempts = Attempts::new(Duration::from_secs(15 * 60));

/// Wrong answers at the step-up prompt (password or code) per session before the session
/// is closed. The first is taken as a typo and gets the prompt back; the second signs out.
pub const REAUTH_MAX_FAILS: u32 = 2;

pub static REAUTH_FAILS: Attempts = Attempts::new(Duration::from_secs(15 * 60));

/// Gate a sensitive handler behind a recent password re-entry.
///
/// Returns `reauth_required`, which the frontend turns into a password prompt and retries.
/// A caller with no session cookie at all is an internal dispatch (MCP, Automator): those
/// never reach a step-up route, because none of them is in the MCP catalog, so the absence
/// of a cookie is treated as "not a browser session" and refused rather than waved through.
pub fn require_step_up(jar: &CookieJar) -> Result<(), ApiError> {
    let Some(key) = session_key(jar) else {
        return Err(ApiError::forbidden_code(
            "reauth_required",
            "this action needs your password",
        ));
    };
    if STEP_UP.holds(&key) {
        Ok(())
    } else {
        Err(ApiError::forbidden_code(
            "reauth_required",
            "this action needs your password",
        ))
    }
}

/// Gate a handler behind a password typed **for this request**: the proof is spent here, so
/// the next call asks again, and the time-boxed grant of [`require_step_up`] does not count.
///
/// Returns `reauth_once_required`, which the frontend answers with the same password prompt
/// as `reauth_required`, posting it as single-use.
pub fn require_password_once(jar: &CookieJar) -> Result<(), ApiError> {
    let refused = || ApiError::forbidden_code("reauth_once_required", "this change needs your password");
    let Some(key) = session_key(jar) else {
        return Err(refused());
    };
    if STEP_UP.take_once(&key) {
        Ok(())
    } else {
        Err(refused())
    }
}

// ── Resource guards ──────────────────────────────────────────────────────────

/// Seconds a handler may take to produce a response. 0 disables the limit.
static REQUEST_TIMEOUT_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(120);

/// Default, and what Settings shows when the stored value is absent.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 120;
/// Accepted range for the setting. The low end still clears any normal API call; the high
/// end is four minutes short of an hour, which is past any request worth waiting on.
pub const MIN_REQUEST_TIMEOUT_SECS: u64 = 10;
pub const MAX_REQUEST_TIMEOUT_SECS: u64 = 3600;

pub fn request_timeout_secs() -> u64 {
    REQUEST_TIMEOUT_SECS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Change the limit at runtime (Settings). Takes effect on the next request; no restart.
pub fn set_request_timeout_secs(secs: u64) {
    REQUEST_TIMEOUT_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

/// Bound how long a handler may run.
///
/// This caps the time to *produce* a response, not the life of a streaming body: an SSE
/// handler returns as soon as it hands over the stream, so live price feeds and log tails
/// are untouched. What it stops is a handler that never returns holding a connection and a
/// database permit forever, which is the shape a slow-loris or a pathological query takes.
///
/// It is a ceiling, not a quota: it does not count requests and never refuses a caller for
/// being busy, so an agent driving the API hard is unaffected.
pub async fn request_timeout(req: Request, next: Next) -> Result<Response, ApiError> {
    let secs = request_timeout_secs();
    if secs == 0 {
        return Ok(next.run(req).await);
    }
    let path = req.uri().path().to_string();
    match tokio::time::timeout(Duration::from_secs(secs), next.run(req)).await {
        Ok(response) => Ok(response),
        Err(_) => {
            tracing::warn!("request timeout after {secs}s: {path}");
            Err(ApiError::timeout(&format!(
                "this request took longer than the {secs}s limit and was stopped. Raise it in \
                 Settings if the work legitimately takes longer."
            )))
        }
    }
}

/// Permits for the endpoints that each take a whole machine for minutes (backtest runs,
/// parameter sweeps, Monte-Carlo). Half the cores, at least one: the rest of the app has to
/// stay answerable while a sweep runs.
fn compute_permits() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        tokio::sync::Semaphore::new((cores / 2).max(1))
    })
}

/// How long a compute request waits for a permit before giving up. Generous on purpose:
/// the point is to serialise heavy work, not to refuse it. A caller only ever sees the
/// refusal if the machine has been saturated for this long.
const COMPUTE_QUEUE_WAIT: Duration = Duration::from_secs(300);

/// Serialise the compute-heavy routes.
///
/// A queue, not a rate limit. Nothing is counted per caller and nothing is refused for
/// being frequent: requests wait their turn and then run in full. What it prevents is N
/// concurrent sweeps each claiming every core, which starves the rest of the API and can
/// take the process down on a small host.
pub async fn compute_gate(req: Request, next: Next) -> Result<Response, ApiError> {
    let permit = tokio::time::timeout(COMPUTE_QUEUE_WAIT, compute_permits().acquire()).await;
    match permit {
        Ok(Ok(_permit)) => Ok(next.run(req).await),
        // The semaphore is never closed; this arm exists only to satisfy the type.
        Ok(Err(_)) => Ok(next.run(req).await),
        Err(_) => Err(ApiError::too_many(
            "the machine has been busy with other backtests for five minutes. Try again once \
             the running ones finish.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trading_unlock_belongs_to_one_session() {
        let u = TradingUnlocks::new();
        assert!(!u.holds("sess"));
        u.grant("sess");
        assert!(u.holds("sess"));
        assert!(!u.holds("other"), "another browser asks for its own code");
        u.grant("other");
        u.revoke_session("sess");
        assert!(!u.holds("sess"), "signing out closes trading again");
        assert!(u.holds("other"), "and only for that session");
        u.clear();
        assert!(!u.holds("other"));
    }

    #[test]
    fn a_single_use_proof_is_spent_once_and_the_grant_never_stands_in_for_it() {
        let s = StepUp::new();
        s.grant("sess");
        assert!(!s.take_once("sess"), "a few-minute grant is not a single-use proof");
        s.grant_once("sess");
        assert!(!s.take_once("other"), "proofs belong to their session");
        assert!(s.take_once("sess"));
        assert!(!s.take_once("sess"), "spent: the next change asks again");
        s.grant_once("sess");
        s.revoke("sess");
        assert!(!s.take_once("sess"), "a revoked session loses its proof too");
    }

    #[test]
    fn attempts_count_within_the_window_and_clear_on_success() {
        let a = Attempts::new(Duration::from_secs(60));
        assert_eq!(a.count("1.2.3.4"), 0);
        assert_eq!(a.record("1.2.3.4"), 1);
        assert_eq!(a.record("1.2.3.4"), 2);
        assert_eq!(a.count("1.2.3.4"), 2);
        assert_eq!(a.count("5.6.7.8"), 0, "keys are independent");
        a.clear("1.2.3.4");
        assert_eq!(a.count("1.2.3.4"), 0);
    }

    #[test]
    fn a_lapsed_window_starts_over() {
        let a = Attempts::new(Duration::from_millis(1));
        a.record("k");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(a.count("k"), 0);
        assert_eq!(a.record("k"), 1);
    }

    #[test]
    fn client_ip_prefers_cloudflare_then_xff_then_direct() {
        let mut h = HeaderMap::new();
        assert_eq!(client_ip(&h), "direct");
        h.insert("x-forwarded-for", "9.9.9.9, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&h), "9.9.9.9");
        h.insert("cf-connecting-ip", "2.2.2.2".parse().unwrap());
        assert_eq!(client_ip(&h), "2.2.2.2");
    }

    /// Drive the middleware through a one-route router, which is how it actually runs.
    async fn origin_check(method: Method, headers: &[(&str, &str)]) -> axum::http::StatusCode {
        use axum::{routing::any, Router};
        use tower::ServiceExt;

        let app = Router::new()
            .route("/x", any(|| async { "ok" }))
            .layer(axum::middleware::from_fn(same_origin));
        let mut req = Request::builder().method(method).uri("/x");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        app.oneshot(req.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn safe_methods_always_pass() {
        let cross = &[("sec-fetch-site", "cross-site")][..];
        assert!(origin_check(Method::GET, cross).await.is_success());
        assert!(origin_check(Method::HEAD, cross).await.is_success());
    }

    #[tokio::test]
    async fn sec_fetch_site_decides_when_present() {
        for site in ["same-origin", "none"] {
            let status = origin_check(Method::POST, &[("sec-fetch-site", site)]).await;
            assert!(status.is_success(), "{site} should pass");
        }
        for site in ["cross-site", "same-site"] {
            let status = origin_check(Method::POST, &[("sec-fetch-site", site)]).await;
            assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{site} should not");
        }
    }

    #[tokio::test]
    async fn origin_must_match_host_when_sec_fetch_site_is_absent() {
        let ok = origin_check(
            Method::POST,
            &[("origin", "https://otw.example"), ("host", "otw.example")],
        )
        .await;
        assert!(ok.is_success());

        let bad = origin_check(
            Method::POST,
            &[("origin", "https://evil.example"), ("host", "otw.example")],
        )
        .await;
        assert_eq!(bad, axum::http::StatusCode::FORBIDDEN);

        // A sibling subdomain is not the same origin.
        let sibling = origin_check(
            Method::POST,
            &[("origin", "https://blog.otw.example"), ("host", "otw.example")],
        )
        .await;
        assert_eq!(sibling, axum::http::StatusCode::FORBIDDEN);
    }

    /// curl, an MCP client and the in-process dispatch send neither header. They also carry
    /// no ambient cookie, so they are not a CSRF vector and must keep working.
    #[tokio::test]
    async fn a_request_with_no_browser_headers_passes() {
        assert!(origin_check(Method::POST, &[]).await.is_success());
        assert!(origin_check(Method::DELETE, &[("host", "otw.example")])
            .await
            .is_success());
    }

    #[test]
    fn session_key_reads_the_https_cookie_too() {
        use axum_extra::extract::cookie::Cookie;
        let plain = CookieJar::new().add(Cookie::new(crate::SESSION_COOKIE, "abc"));
        let https = CookieJar::new().add(Cookie::new(crate::SESSION_COOKIE_HOST, "abc"));
        assert!(session_key(&plain).is_some());
        assert_eq!(session_key(&https), session_key(&plain));
        assert_eq!(session_key(&CookieJar::new()), None);
    }

    #[test]
    fn step_up_is_per_session() {
        let s = StepUp::new();
        s.grant("session-a");
        assert!(s.holds("session-a"));
        assert!(!s.holds("session-b"));
        s.revoke("session-a");
        assert!(!s.holds("session-a"));
    }
}
