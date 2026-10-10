//! Social sign-in protocol: OpenID Connect (Google, Microsoft, any OIDC issuer) and GitHub's
//! OAuth, as an authorization-code flow with PKCE.
//!
//! What the instance accepts is **one identity**: the issuer plus the provider's stable
//! subject id, recorded when the owner links the account from Settings. A different
//! account at the same provider is refused, and so is the same e-mail at another provider.
//! E-mail is display only: providers recycle and change addresses, subjects never move.
//!
//! The ID token is checked on its claims (issuer, audience, expiry, nonce) but not on its
//! signature. That is OIDC Core §3.1.3.7 item 6: a token received directly from the token
//! endpoint over TLS, in a back-channel request this server made with its own client
//! credentials, is authenticated by that TLS connection. It never transits the browser, so
//! there is nothing to forge, and skipping JWKS keeps a key-rotation cache and a JOSE
//! dependency out of the sign-in path.
//!
//! Flows live in memory: an abandoned sign-in should evaporate with the process, and
//! neither a PKCE verifier nor a nonce has any business in the database. Each flow is bound
//! to the browser that started it by a short-lived cookie carrying its `state`, so a
//! callback URL handed to someone else (login CSRF) settles nothing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The SPA route the provider sends the browser back to. This is what the owner registers
/// as the redirect URI, so it must stay stable.
pub const CALLBACK_PATH: &str = "/auth/social";
/// Cookie binding a flow to the browser that started it.
pub const FLOW_COOKIE: &str = "otw_social";
/// A sign-in nobody completes is dropped after this long. Also the cookie's life.
pub const FLOW_TTL: Duration = Duration::from_secs(10 * 60);
/// Flows held at once. The login start is public, so this is what stops a script filling
/// memory with sign-ins it never finishes.
const MAX_FLOWS: usize = 500;
/// Wrong second-factor codes one social sign-in may take before it has to start over.
const MAX_CODE_ATTEMPTS: u32 = 5;
/// Clock skew forgiven on the ID token's expiry.
const EXPIRY_SKEW_SECS: i64 = 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

const GITHUB_ISSUER: &str = "https://github.com";

// ── Providers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    Microsoft,
    Github,
    /// Any OpenID Connect issuer: Authentik, Keycloak, Authelia, Zitadel, Okta…
    Oidc,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Self::Google),
            "microsoft" => Some(Self::Microsoft),
            "github" => Some(Self::Github),
            "oidc" => Some(Self::Oidc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
            Self::Github => "github",
            Self::Oidc => "oidc",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::Github => "read:user",
            _ => "openid email profile",
        }
    }
}

/// What a flow needs to know about the configured provider. `issuer` is the Microsoft
/// tenant (empty means `common`) or the generic issuer URL.
#[derive(Clone)]
pub struct Config {
    pub provider: Provider,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// Endpoints for one sign-in, and the issuer its ID token must carry.
struct Endpoints {
    authorize: String,
    token: String,
    /// Expected `iss`. Microsoft's multi-tenant metadata carries a `{tenantid}` template,
    /// filled from the token's own `tid` claim.
    issuer: String,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
}

/// Normalize a configured issuer URL: trimmed, no trailing slash, `https://` (or `http://`
/// on loopback, for a provider running next to the app).
pub fn normalize_issuer(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    let url = Url::parse(raw).map_err(|_| "the issuer must be a full URL".to_string())?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("the issuer must use https://".into());
    }
    Ok(raw.to_string())
}

/// A Microsoft tenant: `common`, `organizations`, `consumers`, a GUID or a domain.
pub fn valid_tenant(tenant: &str) -> bool {
    !tenant.is_empty()
        && tenant.len() <= 128
        && tenant.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

async fn endpoints(http: &Client, cfg: &Config) -> anyhow::Result<Endpoints> {
    let discovery_url = match cfg.provider {
        Provider::Github => {
            return Ok(Endpoints {
                authorize: "https://github.com/login/oauth/authorize".into(),
                token: "https://github.com/login/oauth/access_token".into(),
                issuer: GITHUB_ISSUER.into(),
            })
        }
        Provider::Google => "https://accounts.google.com/.well-known/openid-configuration".into(),
        Provider::Microsoft => {
            let tenant = cfg.issuer.trim();
            let tenant = if tenant.is_empty() { "common" } else { tenant };
            format!("https://login.microsoftonline.com/{tenant}/v2.0/.well-known/openid-configuration")
        }
        Provider::Oidc => format!("{}/.well-known/openid-configuration", cfg.issuer),
    };
    let res = http
        .get(&discovery_url)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("reaching {discovery_url}"))?;
    if !res.status().is_success() {
        bail!("{discovery_url} answered {}", res.status().as_u16());
    }
    let d: Discovery = res.json().await.context("reading the provider metadata")?;
    // A generic issuer must describe itself as the URL it was configured with, or a typo
    // (or a hostile redirect) would silently move trust to another issuer.
    if cfg.provider == Provider::Oidc && d.issuer.trim_end_matches('/') != cfg.issuer {
        bail!(
            "the provider calls itself `{}`, not `{}`: copy the issuer URL exactly",
            d.issuer,
            cfg.issuer
        );
    }
    Ok(Endpoints {
        authorize: d.authorization_endpoint,
        token: d.token_endpoint,
        issuer: d.issuer,
    })
}

/// Reach the provider's metadata once, so a wrong tenant or issuer is reported when the
/// configuration is saved rather than at the first sign-in.
pub async fn check(http: &Client, cfg: &Config) -> anyhow::Result<()> {
    endpoints(http, cfg).await.map(|_| ())
}

/// The redirect URI for a browser origin, or `None` when it is not an http(s) origin.
pub fn redirect_uri(origin: &str) -> Option<String> {
    let url = Url::parse(origin.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    (origin != "null").then(|| format!("{origin}{CALLBACK_PATH}"))
}

// ── Flows ────────────────────────────────────────────────────────────────────

/// Why the browser went to the provider.
#[derive(Debug, Clone)]
pub enum Purpose {
    /// Sign in; `next` is the same-origin path to land on.
    Login { next: String },
    /// Lock social sign-in to whichever identity comes back, for this account.
    Link { user_id: Uuid },
}

enum Stage {
    /// At the provider, coming back to the callback.
    Away {
        purpose: Purpose,
        cfg: Config,
        token_url: String,
        issuer: String,
        verifier: String,
        nonce: String,
        redirect_uri: String,
    },
    /// Identity proved, the account's second factor still owed.
    AwaitingCode { user_id: Uuid, next: String, attempts: u32 },
}

struct Flow {
    stage: Stage,
    started: Instant,
}

fn flows() -> &'static Mutex<HashMap<String, Flow>> {
    static FLOWS: OnceLock<Mutex<HashMap<String, Flow>>> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random(bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("getrandom: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Start a sign-in. Returns the `state` (also the flow cookie's value) and where to send
/// the browser.
pub async fn start(
    http: &Client,
    cfg: Config,
    purpose: Purpose,
    redirect_uri: &str,
) -> anyhow::Result<(String, String)> {
    let ep = endpoints(http, &cfg).await?;
    let state = random(32)?;
    let verifier = random(48)?;
    let nonce = random(24)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut params = vec![
        ("client_id", cfg.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", cfg.provider.scope()),
        ("state", state.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    if cfg.provider != Provider::Github {
        params.push(("nonce", nonce.as_str()));
    }
    // Linking is the moment the account is chosen, so offer the picker rather than
    // silently taking whoever the browser is signed in as.
    if matches!(purpose, Purpose::Link { .. }) {
        params.push(("prompt", "select_account"));
    }
    let url = Url::parse_with_params(&ep.authorize, &params).context("building the sign-in URL")?;

    let mut guard = flows().lock().unwrap();
    guard.retain(|_, f| f.started.elapsed() < FLOW_TTL);
    if guard.len() >= MAX_FLOWS {
        bail!("too many sign-ins in progress; try again in a few minutes");
    }
    guard.insert(
        state.clone(),
        Flow {
            stage: Stage::Away {
                purpose,
                cfg,
                token_url: ep.token,
                issuer: ep.issuer,
                verifier,
                nonce,
                redirect_uri: redirect_uri.to_string(),
            },
            started: Instant::now(),
        },
    );
    Ok((state, url.to_string()))
}

/// Who the provider says signed in.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
    /// Display only: an e-mail, or `@login` on GitHub.
    pub label: String,
}

/// Redeem the callback's code. The flow is consumed whatever the outcome: a code is single
/// use and a failed exchange starts over. `cookie_state` is the flow cookie; it must match.
pub async fn finish(
    http: &Client,
    state: &str,
    cookie_state: Option<&str>,
    code: &str,
) -> anyhow::Result<(Purpose, Identity)> {
    if cookie_state != Some(state) {
        bail!("this sign-in was started in another browser or has expired; start it again");
    }
    let flow = flows().lock().unwrap().remove(state);
    let Some(Flow { stage: Stage::Away { purpose, cfg, token_url, issuer, verifier, nonce, redirect_uri }, started }) = flow else {
        bail!("this sign-in has expired; start it again");
    };
    if started.elapsed() >= FLOW_TTL {
        bail!("this sign-in has expired; start it again");
    }

    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", cfg.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    if let Some(secret) = cfg.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let res = http
        .post(&token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&form)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .context("completing the sign-in with the provider")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let tokens: TokenResponse = serde_json::from_str(&body).unwrap_or_default();
    // GitHub reports a refused code as a 200 with an `error` field.
    if !status.is_success() || tokens.error.is_some() {
        bail!("{}", describe(&tokens, status.as_u16()));
    }

    let identity = match cfg.provider {
        Provider::Github => {
            let access = tokens
                .access_token
                .ok_or_else(|| anyhow!("GitHub returned no access token"))?;
            github_identity(http, &access).await?
        }
        _ => {
            let id_token = tokens.id_token.ok_or_else(|| {
                anyhow!("the provider returned no ID token: is the `openid` scope allowed?")
            })?;
            let claims = decode_claims(&id_token)?;
            validate(&claims, &issuer, &cfg.client_id, &nonce, now())
                .map_err(|e| anyhow!("{e}"))?
        }
    };
    Ok((purpose, identity))
}

/// Park a proved sign-in until the account's second factor arrives.
pub fn await_code(state: &str, user_id: Uuid, next: String) {
    let mut guard = flows().lock().unwrap();
    if guard.len() < MAX_FLOWS {
        guard.insert(
            state.to_string(),
            Flow {
                stage: Stage::AwaitingCode { user_id, next, attempts: 0 },
                started: Instant::now(),
            },
        );
    }
}

/// The parked sign-in for this flow cookie, if any.
pub fn awaiting(cookie_state: &str) -> Option<(Uuid, String)> {
    let guard = flows().lock().unwrap();
    match guard.get(cookie_state) {
        Some(Flow { stage: Stage::AwaitingCode { user_id, next, .. }, started })
            if started.elapsed() < FLOW_TTL =>
        {
            Some((*user_id, next.clone()))
        }
        _ => None,
    }
}

/// Count a wrong code; past the limit the parked sign-in is dropped.
pub fn code_failed(cookie_state: &str) {
    let mut guard = flows().lock().unwrap();
    let drop = match guard.get_mut(cookie_state) {
        Some(Flow { stage: Stage::AwaitingCode { attempts, .. }, .. }) => {
            *attempts += 1;
            *attempts >= MAX_CODE_ATTEMPTS
        }
        _ => false,
    };
    if drop {
        guard.remove(cookie_state);
    }
}

pub fn forget(cookie_state: &str) {
    flows().lock().unwrap().remove(cookie_state);
}

// ── Token response and ID token ──────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// The provider's own wording, first line only. It names the actual cause (a redirect URI
/// not registered, a wrong secret), which beats anything invented here.
fn describe(t: &TokenResponse, status: u16) -> String {
    match (&t.error_description, &t.error) {
        (Some(d), _) if !d.trim().is_empty() => d.lines().next().unwrap_or_default().trim().into(),
        (_, Some(e)) if !e.is_empty() => e.clone(),
        _ => format!("the provider answered {status}"),
    }
}

#[derive(Deserialize, Debug)]
struct Claims {
    iss: String,
    sub: String,
    #[serde(default)]
    aud: serde_json::Value,
    #[serde(default)]
    azp: Option<String>,
    exp: i64,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    /// Microsoft tenant id, filling the `{tenantid}` template of multi-tenant metadata.
    #[serde(default)]
    tid: Option<String>,
}

fn decode_claims(jwt: &str) -> anyhow::Result<Claims> {
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("the ID token is not a JWT"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .context("decoding the ID token")?;
    serde_json::from_slice(&bytes).context("reading the ID token claims")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Check the claims that make an ID token this sign-in's, and nothing else's.
fn validate(
    c: &Claims,
    expected_issuer: &str,
    client_id: &str,
    nonce: &str,
    now: i64,
) -> Result<Identity, String> {
    let expected = match (expected_issuer.contains("{tenantid}"), c.tid.as_deref()) {
        (true, Some(tid)) => expected_issuer.replace("{tenantid}", tid),
        (true, None) => return Err("the ID token names no tenant".into()),
        (false, _) => expected_issuer.to_string(),
    };
    if c.iss != expected {
        return Err(format!("the ID token comes from `{}`, expected `{expected}`", c.iss));
    }
    let audiences: Vec<&str> = match &c.aud {
        serde_json::Value::String(s) => vec![s.as_str()],
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };
    if !audiences.contains(&client_id) {
        return Err("the ID token was issued to another application".into());
    }
    if audiences.len() > 1 && c.azp.as_deref() != Some(client_id) {
        return Err("the ID token was issued to another application".into());
    }
    if c.exp + EXPIRY_SKEW_SECS < now {
        return Err("the ID token has expired: check this server's clock".into());
    }
    if c.nonce.as_deref() != Some(nonce) {
        return Err("the ID token does not belong to this sign-in".into());
    }
    if c.sub.trim().is_empty() {
        return Err("the ID token names no user".into());
    }
    let label = c
        .email
        .clone()
        .or_else(|| c.preferred_username.clone())
        .unwrap_or_default();
    Ok(Identity { issuer: c.iss.clone(), subject: c.sub.clone(), label })
}

#[derive(Deserialize)]
struct GithubUser {
    id: u64,
    login: String,
}

/// GitHub is OAuth without OpenID Connect: the identity is the numeric user id from the
/// API, which survives a rename of the login.
async fn github_identity(http: &Client, access_token: &str) -> anyhow::Result<Identity> {
    let res = http
        .get("https://api.github.com/user")
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(reqwest::header::USER_AGENT, "OpenTraderWorld")
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .context("reading the GitHub account")?;
    if !res.status().is_success() {
        bail!("GitHub answered {} for the account", res.status().as_u16());
    }
    let user: GithubUser = res.json().await.context("reading the GitHub account")?;
    Ok(Identity {
        issuer: GITHUB_ISSUER.into(),
        subject: user.id.to_string(),
        label: format!("@{}", user.login),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(json: serde_json::Value) -> Claims {
        serde_json::from_value(json).unwrap()
    }

    fn good() -> serde_json::Value {
        serde_json::json!({
            "iss": "https://accounts.google.com",
            "sub": "1234567890",
            "aud": "client-1",
            "exp": 2_000,
            "nonce": "n-1",
            "email": "me@example.com"
        })
    }

    #[test]
    fn a_matching_token_yields_its_subject() {
        let id = validate(&claims(good()), "https://accounts.google.com", "client-1", "n-1", 1_000)
            .unwrap();
        assert_eq!(id.subject, "1234567890");
        assert_eq!(id.issuer, "https://accounts.google.com");
        assert_eq!(id.label, "me@example.com");
    }

    #[test]
    fn issuer_audience_nonce_and_expiry_are_each_enforced() {
        let c = claims(good());
        assert!(validate(&c, "https://evil.example", "client-1", "n-1", 1_000).is_err());
        assert!(validate(&c, "https://accounts.google.com", "client-2", "n-1", 1_000).is_err());
        assert!(validate(&c, "https://accounts.google.com", "client-1", "n-2", 1_000).is_err());
        assert!(validate(&c, "https://accounts.google.com", "client-1", "n-1", 5_000).is_err());
    }

    #[test]
    fn several_audiences_need_the_authorized_party() {
        let mut j = good();
        j["aud"] = serde_json::json!(["client-1", "other"]);
        assert!(validate(&claims(j.clone()), "https://accounts.google.com", "client-1", "n-1", 1_000).is_err());
        j["azp"] = serde_json::json!("client-1");
        assert!(validate(&claims(j), "https://accounts.google.com", "client-1", "n-1", 1_000).is_ok());
    }

    #[test]
    fn microsoft_multi_tenant_issuer_is_filled_from_the_tenant_claim() {
        let template = "https://login.microsoftonline.com/{tenantid}/v2.0";
        let mut j = good();
        j["iss"] = serde_json::json!("https://login.microsoftonline.com/abc-123/v2.0");
        j["tid"] = serde_json::json!("abc-123");
        assert!(validate(&claims(j.clone()), template, "client-1", "n-1", 1_000).is_ok());
        j["tid"] = serde_json::json!("other");
        assert!(validate(&claims(j), template, "client-1", "n-1", 1_000).is_err());
    }

    #[test]
    fn claims_decode_from_the_jwt_payload() {
        let payload = URL_SAFE_NO_PAD.encode(good().to_string());
        let jwt = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        assert_eq!(decode_claims(&jwt).unwrap().sub, "1234567890");
        assert!(decode_claims("not-a-jwt").is_err());
    }

    #[test]
    fn redirect_uri_is_the_origin_plus_the_callback() {
        assert_eq!(
            redirect_uri("https://otw.example/settings"),
            Some("https://otw.example/auth/social".into())
        );
        assert_eq!(
            redirect_uri("http://127.0.0.1:5454"),
            Some("http://127.0.0.1:5454/auth/social".into())
        );
        assert_eq!(redirect_uri("javascript:alert(1)"), None);
        assert_eq!(redirect_uri("nope"), None);
    }

    #[test]
    fn issuers_must_be_https_except_on_loopback() {
        assert_eq!(normalize_issuer(" https://id.example/app/ ").unwrap(), "https://id.example/app");
        assert!(normalize_issuer("http://id.example").is_err());
        assert!(normalize_issuer("http://localhost:9000").is_ok());
        assert!(normalize_issuer("id.example").is_err());
    }

    #[test]
    fn tenants_are_plain_identifiers() {
        assert!(valid_tenant("common"));
        assert!(valid_tenant("contoso.onmicrosoft.com"));
        assert!(!valid_tenant("a/b"));
        assert!(!valid_tenant(""));
    }

    #[test]
    fn a_wrong_browser_cannot_finish_a_flow() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt
            .block_on(finish(&Client::new(), "state-a", Some("state-b"), "code"))
            .unwrap_err();
        assert!(err.to_string().contains("another browser"));
    }
}
