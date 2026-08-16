//! Microsoft OAuth 2.0 for IMAP (Outlook.com and Microsoft 365).
//!
//! Microsoft turned off password authentication for IMAP, so those mailboxes sign in with
//! XOAUTH2. Two flows are offered, in this order:
//!
//! * **Authorization code + PKCE** — what Microsoft recommends for a public client. The
//!   browser comes back to this app's own address, so it needs a redirect URI Microsoft
//!   will accept: any `https://` origin, or `http://` on the loopback interface
//!   (RFC 8252 — the port is ignored when matching a loopback redirect).
//! * **Device code** — the fallback for an instance served over plain HTTP on a LAN
//!   address, which has no registerable redirect URI. Microsoft's own managed Conditional
//!   Access policy blocks device code by default, so on a Microsoft 365 tenant it will
//!   usually be refused; personal Outlook.com accounts still accept it.
//! * **The refresh token is the credential.** It is sealed in the vault like a password;
//!   access tokens live in memory for their hour and are never written down.
//!
//! Microsoft rotates the refresh token on every exchange, so a new one is persisted each
//! time — before it is used, so a crash mid-refresh can never lose the only copy.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use async_imap::Authenticator;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Delegated permissions requested: IMAP access, plus a refresh token.
const SCOPE: &str = "offline_access https://outlook.office.com/IMAP.AccessAsUser.All";
/// Access tokens are re-used until this much of their life is left, then re-fetched.
const EXPIRY_MARGIN: Duration = Duration::from_secs(300);
/// A sign-in that nobody completes is dropped after this long.
const FLOW_TTL: Duration = Duration::from_secs(20 * 60);
/// The SPA route Microsoft sends the browser back to. This is what the user registers as
/// the application's redirect URI, so it must stay stable.
pub const CALLBACK_PATH: &str = "/mailbox/oauth";

fn base(tenant: &str) -> String {
    let tenant = if tenant.trim().is_empty() { "common" } else { tenant.trim() };
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0")
}

// ── Flows in progress ────────────────────────────────────────────────────────

/// How a sign-in is waiting.
enum Kind {
    /// The user is typing a code on Microsoft's page; we poll for the answer.
    Device { device_code: String },
    /// The browser is away at Microsoft and will come back to `CALLBACK_PATH`. `outcome`
    /// is written by the callback and drained by the next poll.
    Code {
        verifier: String,
        redirect_uri: String,
        outcome: Option<Result<(), String>>,
    },
}

/// A sign-in in progress. Held in memory only: an abandoned flow should evaporate with the
/// process, and neither a device code nor a PKCE verifier has any business in a database.
struct Flow {
    account_id: Uuid,
    tenant: String,
    client_id: String,
    kind: Kind,
    started: Instant,
}

fn flows() -> &'static Mutex<HashMap<Uuid, Flow>> {
    static FLOWS: OnceLock<Mutex<HashMap<Uuid, Flow>>> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember a flow (pruning stale ones) and hand back its handle.
fn remember(flow: Flow) -> Uuid {
    let handle = Uuid::new_v4();
    let mut guard = flows().lock().unwrap();
    guard.retain(|_, f| f.started.elapsed() < FLOW_TTL);
    guard.insert(handle, flow);
    handle
}

// ── Authorization code + PKCE ────────────────────────────────────────────────

/// Turn the browser's own origin into a redirect URI Microsoft will accept, or `None` when
/// it cannot be registered at all — a plain-HTTP LAN address, which is exactly the case the
/// device-code fallback exists for.
pub fn redirect_uri(origin: &str) -> Option<String> {
    let url = Url::parse(origin.trim()).ok()?;
    let host = url.host_str()?;
    // `[::1]` is explicitly not supported by Microsoft, so it is not loopback here.
    let loopback = host == "localhost" || host == "127.0.0.1";
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        return None;
    }
    Some(format!("{origin}{CALLBACK_PATH}"))
}

/// Where to send the browser, and the redirect it will come back to.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeSignIn {
    pub authorize_url: String,
    pub redirect_uri: String,
}

fn random_secret() -> anyhow::Result<String> {
    let mut buf = [0u8; 48];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("getrandom: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Build an authorization-code sign-in. No network call: the browser makes the request.
/// The handle doubles as the OAuth `state`, so a callback can only settle the flow it was
/// actually issued for.
pub fn start_code(
    account_id: Uuid,
    tenant: &str,
    client_id: &str,
    login_hint: &str,
    redirect_uri: &str,
) -> anyhow::Result<(Uuid, CodeSignIn)> {
    let verifier = random_secret()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let handle = remember(Flow {
        account_id,
        tenant: tenant.to_string(),
        client_id: client_id.to_string(),
        kind: Kind::Code {
            verifier,
            redirect_uri: redirect_uri.to_string(),
            outcome: None,
        },
        started: Instant::now(),
    });

    let state = handle.to_string();
    let mut params = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("response_mode", "query"),
        ("scope", SCOPE),
        ("state", state.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        // The mailbox address is fixed by the account, so offer the account picker rather
        // than silently signing in as whoever the browser saw last.
        ("prompt", "select_account"),
    ];
    if !login_hint.trim().is_empty() {
        params.push(("login_hint", login_hint.trim()));
    }
    let url = Url::parse_with_params(&format!("{}/authorize", base(tenant)), &params)
        .context("building the sign-in URL")?;
    Ok((
        handle,
        CodeSignIn {
            authorize_url: url.to_string(),
            redirect_uri: redirect_uri.to_string(),
        },
    ))
}

/// Exchange the authorization code the callback came back with. Returns the account it
/// belongs to and its refresh token; the caller seals that and then calls `settle`.
pub async fn redeem(http: &Client, handle: Uuid, code: &str) -> anyhow::Result<(Uuid, String)> {
    let (account_id, tenant, client_id, verifier, redirect_uri) = {
        let guard = flows().lock().unwrap();
        let flow = guard
            .get(&handle)
            .ok_or_else(|| anyhow!("this sign-in expired — start it again"))?;
        match &flow.kind {
            Kind::Code { verifier, redirect_uri, .. } => (
                flow.account_id,
                flow.tenant.clone(),
                flow.client_id.clone(),
                verifier.clone(),
                redirect_uri.clone(),
            ),
            Kind::Device { .. } => {
                return Err(anyhow!("this sign-in is waiting for a code, not a redirect"))
            }
        }
    };

    let res = http
        .post(format!("{}/token", base(&tenant)))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("scope", SCOPE),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("completing the sign-in")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("{}", describe(&body, status.as_u16())));
    }
    let tokens: TokenResponse =
        serde_json::from_str(&body).context("reading the token response")?;
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        anyhow!("Microsoft returned no refresh token — is offline_access allowed for this app?")
    })?;
    Ok((account_id, refresh_token))
}

/// Record how a redirect sign-in ended, so the window that started it stops waiting.
pub fn settle(handle: Uuid, outcome: Result<(), String>) {
    if let Some(flow) = flows().lock().unwrap().get_mut(&handle) {
        if let Kind::Code { outcome: slot, .. } = &mut flow.kind {
            *slot = Some(outcome);
        }
    }
}

// ── Device-code sign-in ──────────────────────────────────────────────────────

/// What the user must do, in their own browser, on any device.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}
fn default_interval() -> u64 {
    5
}

/// Ask Microsoft for a device code and remember the flow. Returns its handle plus what to
/// show the user.
pub async fn start_device(
    http: &Client,
    account_id: Uuid,
    tenant: &str,
    client_id: &str,
) -> anyhow::Result<(Uuid, DeviceCode)> {
    let res = http
        .post(format!("{}/devicecode", base(tenant)))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("requesting a device code")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("{}", describe(&body, status.as_u16())));
    }
    let parsed: DeviceCodeResponse =
        serde_json::from_str(&body).context("reading the device-code response")?;

    let handle = remember(Flow {
        account_id,
        tenant: tenant.to_string(),
        client_id: client_id.to_string(),
        kind: Kind::Device { device_code: parsed.device_code },
        started: Instant::now(),
    });
    Ok((
        handle,
        DeviceCode {
            user_code: parsed.user_code,
            verification_uri: parsed.verification_uri,
            expires_in: parsed.expires_in,
            interval: parsed.interval,
        },
    ))
}

// ── Polling ──────────────────────────────────────────────────────────────────

/// Where a sign-in stands.
pub enum Progress {
    /// Still waiting for the user to approve, poll again in `interval` seconds.
    Pending { interval: u64 },
    /// Approved. `refresh_token` carries the credential when the poll itself obtained it
    /// (device code); a redirect sign-in was already sealed by its callback, and sends
    /// `None`.
    Done { refresh_token: Option<String>, account_id: Uuid },
    /// Declined, expired, or refused by a tenant policy — with Microsoft's own wording.
    Failed(String),
}

/// What `poll` has to do once the lock is released.
enum Step {
    /// A redirect sign-in that has already answered, or not yet.
    Settled(Progress),
    /// A device-code sign-in: tenant, client id, device code.
    Ask(String, String, String),
}

/// Poll one flow once. `Failed` and `Done` both retire the flow.
pub async fn poll(http: &Client, handle: Uuid) -> anyhow::Result<Progress> {
    // The lock is confined to this block: nothing is held across an await.
    let (account_id, step) = {
        let mut guard = flows().lock().unwrap();
        let flow = guard
            .get_mut(&handle)
            .ok_or_else(|| anyhow!("this sign-in expired — start it again"))?;
        let account_id = flow.account_id;
        let step = match &mut flow.kind {
            Kind::Device { device_code } => Step::Ask(
                flow.tenant.clone(),
                flow.client_id.clone(),
                device_code.clone(),
            ),
            Kind::Code { outcome, .. } => Step::Settled(match outcome.take() {
                None => Progress::Pending { interval: 2 },
                Some(Ok(())) => Progress::Done { refresh_token: None, account_id },
                Some(Err(message)) => Progress::Failed(message),
            }),
        };
        (account_id, step)
    };

    let (tenant, client_id, device_code) = match step {
        Step::Settled(progress @ Progress::Pending { .. }) => return Ok(progress),
        Step::Settled(progress) => {
            flows().lock().unwrap().remove(&handle);
            return Ok(progress);
        }
        Step::Ask(tenant, client_id, device_code) => (tenant, client_id, device_code),
    };

    let res = http
        .post(format!("{}/token", base(&tenant)))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", device_code.as_str()),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("checking the sign-in")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();

    if status.is_success() {
        let tokens: TokenResponse =
            serde_json::from_str(&body).context("reading the token response")?;
        flows().lock().unwrap().remove(&handle);
        let refresh_token = tokens
            .refresh_token
            .ok_or_else(|| anyhow!("Microsoft returned no refresh token — is offline_access allowed for this app?"))?;
        return Ok(Progress::Done { refresh_token: Some(refresh_token), account_id });
    }

    match error_code(&body).as_str() {
        "authorization_pending" => Ok(Progress::Pending { interval: 5 }),
        // The server asks us to back off; the UI just waits longer.
        "slow_down" => Ok(Progress::Pending { interval: 10 }),
        _ => {
            flows().lock().unwrap().remove(&handle);
            Ok(Progress::Failed(describe(&body, status.as_u16())))
        }
    }
}

// ── Access tokens ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default = "default_expiry")]
    expires_in: u64,
}
fn default_expiry() -> u64 {
    3600
}

/// Outcome of exchanging a refresh token.
pub struct Refreshed {
    pub access_token: String,
    /// Microsoft rotates refresh tokens; store this one when present.
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

/// Why a refresh failed. The distinction is the whole point: a network blip must not
/// unlink a mailbox, and a revoked grant must not be retried every minute.
pub enum RefreshError {
    /// The grant is gone: password change, MFA reset, admin revocation, consent withdrawn,
    /// or 90 days without use. Only the user can fix it.
    Revoked(String),
    /// Anything else (offline, 5xx, timeout) — worth retrying on the next poll.
    Transient(anyhow::Error),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::Revoked(m) => write!(f, "{m}"),
            RefreshError::Transient(e) => write!(f, "{e:#}"),
        }
    }
}

/// Exchange a refresh token for an access token.
pub async fn refresh(
    http: &Client,
    tenant: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<Refreshed, RefreshError> {
    let res = http
        .post(format!("{}/token", base(tenant)))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("scope", SCOPE),
            ("refresh_token", refresh_token),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| RefreshError::Transient(anyhow!("reaching Microsoft: {e}")))?;

    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if status.is_success() {
        let t: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| RefreshError::Transient(anyhow!("reading the token response: {e}")))?;
        return Ok(Refreshed {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            expires_in: t.expires_in,
        });
    }

    let code = error_code(&body);
    let message = describe(&body, status.as_u16());
    // `invalid_grant` / `invalid_client` mean the credential itself is finished; a 5xx or a
    // throttle is the network having a bad day.
    if code == "invalid_grant" || code == "invalid_client" || code == "unauthorized_client" {
        Err(RefreshError::Revoked(message))
    } else if status.is_server_error() || code == "temporarily_unavailable" {
        Err(RefreshError::Transient(anyhow!("{message}")))
    } else {
        Err(RefreshError::Revoked(message))
    }
}

/// Per-account access-token cache. An access token is good for about an hour and polls run
/// every few minutes, so this saves a token round-trip (and a vault write) per poll.
fn cache() -> &'static Mutex<HashMap<Uuid, (Arc<str>, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<Uuid, (Arc<str>, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cached_token(account_id: Uuid) -> Option<String> {
    let guard = cache().lock().unwrap();
    guard
        .get(&account_id)
        .filter(|(_, until)| *until > Instant::now())
        .map(|(tok, _)| tok.to_string())
}

pub fn cache_token(account_id: Uuid, token: &str, expires_in: u64) {
    let life = Duration::from_secs(expires_in).saturating_sub(EXPIRY_MARGIN);
    cache()
        .lock()
        .unwrap()
        .insert(account_id, (Arc::from(token), Instant::now() + life));
}

pub fn forget_token(account_id: Uuid) {
    cache().lock().unwrap().remove(&account_id);
}

// ── SASL XOAUTH2 ─────────────────────────────────────────────────────────────

/// SASL XOAUTH2 initial response: `user=…^Aauth=Bearer …^A^A`.
///
/// On failure the server sends a second challenge carrying a JSON error and expects an
/// empty client response before it will report the tagged NO — hence the `sent` latch.
pub struct XOAuth2 {
    payload: String,
    sent: bool,
}

impl XOAuth2 {
    pub fn new(user: &str, access_token: &str) -> Self {
        Self { payload: format!("user={user}\x01auth=Bearer {access_token}\x01\x01"), sent: false }
    }
}

impl Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        if self.sent {
            String::new()
        } else {
            self.sent = true;
            self.payload.clone()
        }
    }
}

// ── Error shaping ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

fn error_code(body: &str) -> String {
    serde_json::from_str::<ErrorBody>(body).map(|e| e.error).unwrap_or_default()
}

/// Microsoft's own error text, trimmed to its first line. It names the actual cause
/// ("AADSTS50076: due to a configuration change…", "AADSTS7000218: public client flows
/// not enabled"), which is far more useful than anything this module could invent.
fn describe(body: &str, status: u16) -> String {
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(e) if !e.error_description.is_empty() => {
            e.error_description.lines().next().unwrap_or_default().trim().to_string()
        }
        Ok(e) if !e.error.is_empty() => e.error,
        _ => format!("Microsoft answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoauth2_sends_the_payload_once() {
        let mut a = XOAuth2::new("me@example.com", "TOK");
        assert_eq!(a.process(b""), "user=me@example.com\x01auth=Bearer TOK\x01\x01");
        // The failure challenge must be answered with an empty line, not the payload again.
        assert_eq!(a.process(b"{\"status\":\"401\"}"), "");
    }

    #[test]
    fn error_shaping_prefers_microsofts_wording() {
        let body = r#"{"error":"invalid_grant","error_description":"AADSTS50173: token revoked\r\nTrace ID: x"}"#;
        assert_eq!(error_code(body), "invalid_grant");
        assert_eq!(describe(body, 400), "AADSTS50173: token revoked");
        assert_eq!(describe("not json", 503), "Microsoft answered 503");
    }

    #[test]
    fn redirect_uri_accepts_https_and_loopback_only() {
        assert_eq!(
            redirect_uri("http://127.0.0.1:5454"),
            Some("http://127.0.0.1:5454/mailbox/oauth".into())
        );
        assert_eq!(
            redirect_uri("https://mail.example.com/"),
            Some("https://mail.example.com/mailbox/oauth".into())
        );
        // Plain HTTP on a LAN address is not registerable — device code is the fallback.
        assert_eq!(redirect_uri("http://192.168.1.10:5454"), None);
        // Microsoft does not support the IPv6 loopback literal.
        assert_eq!(redirect_uri("http://[::1]:5454"), None);
        assert_eq!(redirect_uri("not a url"), None);
    }

    #[test]
    fn pkce_challenge_is_the_sha256_of_the_verifier() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn tenant_defaults_to_common() {
        assert!(base("  ").ends_with("/common/oauth2/v2.0"));
        assert!(base("contoso.onmicrosoft.com").contains("contoso.onmicrosoft.com"));
    }
}
