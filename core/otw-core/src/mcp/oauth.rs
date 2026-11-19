//! OAuth 2.1 for the MCP gateway (MCP authorization spec, 2025-06-18 and later).
//!
//! OTW is both the authorization server and the protected resource, single user:
//!
//! * Discovery: `/.well-known/oauth-protected-resource[/api/mcp]` (RFC 9728) points at
//!   `/.well-known/oauth-authorization-server` (RFC 8414). Both are public and say nothing
//!   secret, but are only served while OAuth is switched on.
//! * `POST /api/mcp/oauth/register`: dynamic client registration (RFC 7591), open by
//!   design (that is how MCP clients find their way in), throttled and capped.
//! * `/oauth/authorize` is an SPA page: it needs a signed-in session, shows the client and
//!   its redirect host, and lets the owner pick the module permissions. Approving costs a
//!   password (step-up), as minting a token in Settings does.
//! * `POST /api/mcp/oauth/token`: authorization code with PKCE S256 (mandatory), and
//!   refresh with rotation; a spent refresh token presented again revokes the grant.
//! * `POST /api/mcp/oauth/revoke` (RFC 7009).
//!
//! The approval becomes a grant row in `mcp_tokens`, so every later check (permissions,
//! catalog allowlist, call log, revocation in Settings) is the bearer token's own.
//!
//! Gated twice: the gateway switch (`mcp_enabled`) and `mcp_oauth_enabled`, off by
//! default. With OAuth off, the discovery documents 404 and access tokens are refused.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Form, Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{security_events, ApiError, AppState};

/// The path of the protected resource, appended to the public origin.
pub const RESOURCE_PATH: &str = "/api/mcp";
/// The SPA page that asks the owner.
const AUTHORIZE_PAGE: &str = "/oauth/authorize";

/// Registrations allowed per window, across all callers.
const REGISTER_LIMIT: u32 = 20;
const REGISTER_WINDOW: Duration = Duration::from_secs(600);

// ── Switches and addresses ───────────────────────────────────────────────────

/// OAuth is live only when both the gateway and OAuth itself are on.
pub async fn enabled(state: &AppState) -> bool {
    let gw = otw_store::settings::get_or(&state.pool, "mcp_enabled", "false").await;
    let oa = otw_store::settings::get_or(&state.pool, "mcp_oauth_enabled", "false").await;
    matches!(gw.as_deref(), Ok("true")) && matches!(oa.as_deref(), Ok("true"))
}

/// The origin the client reached us at, as Caddy forwarded it. The issuer and the resource
/// are both derived from it, so a client that came in by the LAN address and one that came
/// in by the domain each see a consistent set of URLs.
///
/// In the HTTPS network modes the scheme is https whatever the hop says: a TLS-terminating
/// proxy in front of Caddy (a tunnel) reaches it over plain HTTP, and an `http://` issuer
/// is refused by every remote client.
pub async fn origin(state: &AppState, headers: &HeaderMap) -> String {
    let mode = crate::network_api::effective_mode(&state.pool).await;
    public_origin(headers, matches!(mode.as_str(), "web" | "lan_https"))
}

pub fn public_origin(headers: &HeaderMap, force_https: bool) -> String {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let proto = match first("x-forwarded-proto") {
        _ if force_https => "https",
        Some("https") => "https",
        _ => "http",
    };
    let host = first("x-forwarded-host")
        .or_else(|| first("host"))
        .filter(|h| h.bytes().all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b)))
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

/// `WWW-Authenticate` for a 401 from the gateway: tells an OAuth-capable client where to
/// start. `error` is set when a token was presented and refused (RFC 6750 §3).
pub fn www_authenticate(origin: &str, error: Option<&str>) -> HeaderValue {
    let mut v = format!("Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource{RESOURCE_PATH}\"");
    if let Some(e) = error {
        v.push_str(&format!(", error=\"{e}\""));
    }
    HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static("Bearer"))
}

// ── Routes ───────────────────────────────────────────────────────────────────

/// Public routes: discovery, registration, token, revocation. Any origin may call them
/// (a browser-based MCP client does): they carry no cookie and accept none.
pub fn public_routes() -> Router<AppState> {
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("mcp-protocol-version"),
        ]);
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-protected-resource/api/mcp", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(authorization_server))
        .route("/.well-known/oauth-authorization-server/api/mcp", get(authorization_server))
        .route("/api/mcp/oauth/register", post(register))
        .route("/api/mcp/oauth/token", post(token))
        .route("/api/mcp/oauth/revoke", post(revoke))
        .layer(cors)
}

/// Session routes behind the consent page.
pub fn session_routes() -> Router<AppState> {
    Router::new()
        .route("/api/mcp/oauth/request", get(describe_request))
        .route("/api/mcp/oauth/approve", post(approve))
        .route("/api/mcp/oauth/deny", post(deny))
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "OAuth is disabled on this instance" })))
        .into_response()
}

fn no_store(mut resp: Response) -> Response {
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut().insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    resp
}

/// RFC 6749 §5.2 error body.
fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    no_store(
        (status, Json(json!({ "error": error, "error_description": description }))).into_response(),
    )
}

fn internal(e: anyhow::Error) -> Response {
    tracing::error!("mcp oauth: {e:#}");
    oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "internal error")
}

// ── Discovery ────────────────────────────────────────────────────────────────

async fn protected_resource(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !enabled(&state).await {
        return not_found();
    }
    let origin = origin(&state, &headers).await;
    Json(json!({
        "resource": format!("{origin}{RESOURCE_PATH}"),
        "authorization_servers": [origin],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp"],
        "resource_name": "OpenTraderWorld",
    }))
    .into_response()
}

async fn authorization_server(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !enabled(&state).await {
        return not_found();
    }
    let origin = origin(&state, &headers).await;
    Json(json!({
        "issuer": origin,
        "authorization_endpoint": format!("{origin}{AUTHORIZE_PAGE}"),
        "token_endpoint": format!("{origin}/api/mcp/oauth/token"),
        "registration_endpoint": format!("{origin}/api/mcp/oauth/register"),
        "revocation_endpoint": format!("{origin}/api/mcp/oauth/revoke"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "revocation_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "scopes_supported": ["mcp"],
        "authorization_response_iss_parameter_supported": true,
    }))
    .into_response()
}

// ── Registration ─────────────────────────────────────────────────────────────

static REGISTRATIONS: Mutex<Option<(Instant, u32)>> = Mutex::new(None);

fn registration_allowed() -> bool {
    let mut guard = REGISTRATIONS.lock().unwrap();
    let next = match *guard {
        Some((start, n)) if start.elapsed() < REGISTER_WINDOW => (start, n + 1),
        _ => (Instant::now(), 1),
    };
    *guard = Some(next);
    next.1 <= REGISTER_LIMIT
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Private-use schemes of the native MCP clients known to need one. Everything else uses
/// https or the loopback; a new client with its own scheme is added here deliberately.
const NATIVE_SCHEMES: &[&str] = &["cursor", "vscode", "vscode-insiders"];

/// A redirect URI a client may register: https anywhere, http only to the loopback
/// (RFC 8252 §7.3), or a known native client's scheme. Never a fragment.
pub fn redirect_uri_allowed(uri: &str) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    if url.fragment().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => url.host_str().is_some_and(is_loopback),
        scheme => NATIVE_SCHEMES.contains(&scheme),
    }
}

/// Whether `presented` is one of the client's registered URIs. Exact match, except that a
/// loopback URI may come back on any port: native clients bind whatever port is free.
pub fn redirect_uri_matches(registered: &[String], presented: &str) -> bool {
    if registered.iter().any(|r| r == presented) {
        return true;
    }
    let Ok(p) = Url::parse(presented) else {
        return false;
    };
    if p.scheme() != "http" || !p.host_str().is_some_and(is_loopback) {
        return false;
    }
    registered.iter().filter_map(|r| Url::parse(r).ok()).any(|r| {
        r.scheme() == "http"
            && r.host_str() == p.host_str()
            && r.path() == p.path()
            && r.query() == p.query()
    })
}

#[derive(Deserialize)]
struct RegisterBody {
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
}

async fn register(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    if !enabled(&state).await {
        return not_found();
    }
    let Ok(input) = serde_json::from_slice::<RegisterBody>(&body) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "body must be a JSON object");
    };
    if input.redirect_uris.is_empty() || input.redirect_uris.len() > 10 {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "between one and ten redirect_uris are required",
        );
    }
    if let Some(bad) = input.redirect_uris.iter().find(|u| u.len() > 2000 || !redirect_uri_allowed(u)) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            &format!(
                "redirect URI not allowed: {bad} (https, http on localhost, or {})",
                NATIVE_SCHEMES.join("/")
            ),
        );
    }
    let method = input.token_endpoint_auth_method.as_deref().unwrap_or("none");
    let confidential = match method {
        "none" => false,
        "client_secret_post" | "client_secret_basic" => true,
        other => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                &format!("unsupported token_endpoint_auth_method: {other}"),
            )
        }
    };
    if !registration_allowed() {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "temporarily_unavailable", "too many registrations, retry later");
    }
    let name: String = input
        .client_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("MCP client")
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    match otw_store::mcp_oauth::register_client(&state.pool, &name, &input.redirect_uris, confidential).await {
        Ok(Some((client_id, secret))) => {
            security_events::log(
                security_events::MCP_TOKEN_CREATED,
                &format!("OAuth client registered: {name}"),
            );
            let mut out = json!({
                "client_id": client_id,
                "client_id_issued_at": time::OffsetDateTime::now_utc().unix_timestamp(),
                "client_name": name,
                "redirect_uris": input.redirect_uris,
                "token_endpoint_auth_method": method,
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
            });
            if let Some(s) = secret {
                out["client_secret"] = json!(s);
                out["client_secret_expires_at"] = json!(0);
            }
            no_store((StatusCode::CREATED, Json(out)).into_response())
        }
        Ok(None) => oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "too many registered clients; revoke unused connections in Settings → AI agents",
        ),
        Err(e) => internal(e),
    }
}

// ── Authorization (consent page) ─────────────────────────────────────────────

/// The authorization request as the client sent it, relayed by the consent page.
#[derive(Deserialize, Clone)]
pub struct AuthorizeParams {
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    resource: Option<String>,
}

/// A request checked far enough to know where to send the answer.
struct Checked {
    client: otw_store::mcp_oauth::OauthClient,
    redirect_uri: String,
}

/// Refusals before the redirect URI is trusted are shown on the page; nothing is sent to
/// an address the client did not register (RFC 6749 §4.1.2.1).
fn check_client(
    client: Option<otw_store::mcp_oauth::OauthClient>,
    p: &AuthorizeParams,
) -> Result<Checked, ApiError> {
    let client = client.ok_or_else(|| {
        ApiError::bad_request("unknown client: it may have been removed; reconnect from the client")
    })?;
    let redirect_uri = match &p.redirect_uri {
        Some(uri) => uri.clone(),
        None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
        None => return Err(ApiError::bad_request("redirect_uri is required")),
    };
    if !redirect_uri_matches(&client.redirect_uris, &redirect_uri) {
        return Err(ApiError::bad_request("redirect_uri does not match the client's registration"));
    }
    Ok(Checked { client, redirect_uri })
}

/// Refusals the client is told about through its redirect: (error, description).
fn check_request(p: &AuthorizeParams) -> Result<(), (&'static str, String)> {
    if p.response_type != "code" {
        return Err(("unsupported_response_type", "only response_type=code is supported".into()));
    }
    if p.code_challenge_method.as_deref() != Some("S256") {
        return Err(("invalid_request", "PKCE with code_challenge_method=S256 is required".into()));
    }
    let len = p.code_challenge.len();
    if !(43..=128).contains(&len)
        || !p.code_challenge.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err(("invalid_request", "code_challenge is malformed".into()));
    }
    if let Some(r) = &p.resource {
        if !resource_ok(r) {
            return Err(("invalid_target", format!("unknown resource: {r}")));
        }
    }
    Ok(())
}

/// RFC 8707: the only resource here is the MCP endpoint, whichever host it was reached by.
fn resource_ok(resource: &str) -> bool {
    Url::parse(resource)
        .ok()
        .is_some_and(|u| u.path().trim_end_matches('/') == RESOURCE_PATH && u.fragment().is_none())
}

fn redirect_with(uri: &str, params: &[(&str, &str)]) -> String {
    match Url::parse(uri) {
        Ok(mut url) => {
            {
                let mut q = url.query_pairs_mut();
                for (k, v) in params {
                    q.append_pair(k, v);
                }
            }
            url.to_string()
        }
        Err(_) => uri.to_string(),
    }
}

async fn load_checked(state: &AppState, p: &AuthorizeParams) -> Result<Checked, ApiError> {
    if !enabled(state).await {
        return Err(ApiError::forbidden(
            "OAuth is off: turn it on in Settings → AI agents, then reconnect from the client",
        ));
    }
    let client = otw_store::mcp_oauth::get_client(&state.pool, &p.client_id)
        .await
        .map_err(|e| {
            tracing::error!("mcp oauth: {e:#}");
            ApiError::internal("internal error")
        })?;
    check_client(client, p)
}

fn drop_step_up(jar: &CookieJar) {
    if let Some(key) = crate::security::session_key(jar) {
        crate::security::STEP_UP.revoke(&key);
    }
}

/// `GET /api/mcp/oauth/request`: what the consent page shows. An error here is shown as
/// is; a request error the client should hear about comes back as `redirect`.
async fn describe_request(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(p): Query<AuthorizeParams>,
) -> Result<Json<Value>, ApiError> {
    // Every approval asks for the password, even inside the few minutes a recent one
    // would cover: a consent link can arrive from anyone, and the prompt is the moment the
    // owner stops to read who is asking.
    drop_step_up(&jar);
    let checked = load_checked(&state, &p).await?;
    let redirect_host = Url::parse(&checked.redirect_uri)
        .ok()
        .map(|u| match u.host_str() {
            Some(h) => h.to_string(),
            None => format!("{}:", u.scheme()),
        })
        .unwrap_or_default();
    let error_redirect = check_request(&p).err().map(|(e, d)| {
        let mut params = vec![("error", e), ("error_description", d.as_str())];
        if let Some(s) = &p.state {
            params.push(("state", s));
        }
        redirect_with(&checked.redirect_uri, &params)
    });
    Ok(Json(json!({
        "client_name": checked.client.name,
        "redirect_host": redirect_host,
        "error_redirect": error_redirect,
    })))
}

#[derive(Deserialize)]
struct ApproveBody {
    #[serde(flatten)]
    params: AuthorizeParams,
    name: String,
    permissions: Value,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

/// `POST /api/mcp/oauth/approve`: the owner said yes. Costs a password, like minting a
/// token in Settings: this is the same credential, delivered to someone else.
async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<ApproveBody>,
) -> Result<Json<Value>, ApiError> {
    crate::security::require_step_up(&jar)?;
    let checked = load_checked(&state, &body.params).await?;
    if let Err((_, d)) = check_request(&body.params) {
        return Err(ApiError::bad_request(&d));
    }
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("a name is required"));
    }
    crate::mcp_api::validate_permissions(&body.permissions)?;
    if body.permissions.as_object().is_none_or(|m| m.is_empty()) {
        return Err(ApiError::bad_request("grant at least one module"));
    }
    let grant_expires_at = crate::mcp_api::expiry_from_days(body.expires_in_days)?;
    let approval = otw_store::mcp_oauth::Approval {
        client_id: checked.client.id.clone(),
        redirect_uri: checked.redirect_uri.clone(),
        code_challenge: body.params.code_challenge.clone(),
        resource: body.params.resource.clone(),
        name: name.chars().take(80).collect(),
        permissions: body.permissions.clone(),
        grant_expires_at,
    };
    let code = otw_store::mcp_oauth::create_code(&state.pool, &approval).await.map_err(|e| {
        tracing::error!("mcp oauth: {e:#}");
        ApiError::internal("internal error")
    })?;
    security_events::alert(
        &state,
        security_events::MCP_TOKEN_CREATED,
        &format!("{}:oauth:{}", security_events::MCP_TOKEN_CREATED, checked.client.id),
        "An agent was connected through OAuth",
        &format!(
            "Client: {}\nName: {name}\nRedirect: {}\nModules: {}\nExpires: {}",
            checked.client.name,
            checked.redirect_uri,
            crate::mcp_api::perms_summary(&body.permissions),
            grant_expires_at.map_or("never".to_string(), |at| at.date().to_string()),
        ),
    );
    // Spent: the next approval asks again.
    drop_step_up(&jar);
    let iss = origin(&state, &headers).await;
    let mut params = vec![("code", code.as_str()), ("iss", iss.as_str())];
    if let Some(s) = &body.params.state {
        params.push(("state", s));
    }
    Ok(Json(json!({ "redirect": redirect_with(&checked.redirect_uri, &params) })))
}

/// `POST /api/mcp/oauth/deny`: the owner said no; the client hears `access_denied`.
async fn deny(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(p): Json<AuthorizeParams>,
) -> Result<Json<Value>, ApiError> {
    let checked = load_checked(&state, &p).await?;
    let iss = origin(&state, &headers).await;
    let mut params = vec![
        ("error", "access_denied"),
        ("error_description", "the owner declined the request"),
        ("iss", iss.as_str()),
    ];
    if let Some(s) = &p.state {
        params.push(("state", s));
    }
    Ok(Json(json!({ "redirect": redirect_with(&checked.redirect_uri, &params) })))
}

// ── Token endpoint ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenForm {
    #[serde(default)]
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    resource: Option<String>,
    /// Revocation only.
    #[serde(default)]
    token: Option<String>,
}

/// `client_secret_basic` credentials, if the request carries them.
fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    use base64::Engine;
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(raw.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (id, secret) = text.split_once(':')?;
    Some((url_decode(id), url_decode(secret)))
}

/// RFC 6749 §2.3.1 form-encodes the id and secret before Basic; undo that.
fn url_decode(s: &str) -> String {
    let pairs: Vec<(String, String)> =
        Url::parse(&format!("x:?v={s}")).ok().map(|u| u.query_pairs().into_owned().collect()).unwrap_or_default();
    pairs.into_iter().next().map(|(_, v)| v).unwrap_or_else(|| s.to_string())
}

/// Authenticate the client at the token endpoint. A public client names itself; a
/// confidential one must prove its secret, by Basic or in the body.
async fn authenticate_client(
    state: &AppState,
    headers: &HeaderMap,
    form: &TokenForm,
) -> Result<otw_store::mcp_oauth::OauthClient, Response> {
    let basic = basic_credentials(headers);
    let (id, secret) = match &basic {
        Some((id, secret)) => (Some(id.clone()), Some(secret.clone())),
        None => (form.client_id.clone(), form.client_secret.clone()),
    };
    let invalid = || oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", "client authentication failed");
    let Some(id) = id else {
        return Err(invalid());
    };
    let client = match otw_store::mcp_oauth::get_client(&state.pool, &id).await {
        Ok(Some(c)) => c,
        Ok(None) => return Err(invalid()),
        Err(e) => return Err(internal(e)),
    };
    if let Some(expected) = &client.secret_hash {
        let presented = secret.as_deref().map(otw_store::mcp::hash_token).unwrap_or_default();
        if !bool::from(presented.as_bytes().ct_eq(expected.as_bytes())) {
            return Err(invalid());
        }
    }
    Ok(client)
}

/// PKCE S256: BASE64URL(SHA256(verifier)) == challenge, compared in constant time.
pub fn pkce_ok(verifier: &str, challenge: &str) -> bool {
    use base64::Engine;
    if !(43..=128).contains(&verifier.len())
        || !verifier.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return false;
    }
    let digest = Sha256::digest(verifier.as_bytes());
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    computed.as_bytes().ct_eq(challenge.as_bytes()).into()
}

fn token_response(access: String, refresh: String) -> Response {
    no_store(
        Json(json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": otw_store::mcp_oauth::ACCESS_TTL.whole_seconds(),
            "refresh_token": refresh,
            "scope": "mcp",
        }))
        .into_response(),
    )
}

async fn token(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<TokenForm>) -> Response {
    if !enabled(&state).await {
        return not_found();
    }
    let client = match authenticate_client(&state, &headers, &form).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    if let Some(r) = &form.resource {
        if !resource_ok(r) {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_target", "unknown resource");
        }
    }
    match form.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(verifier)) = (&form.code, &form.code_verifier) else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "code and code_verifier are required");
            };
            let approval = match otw_store::mcp_oauth::take_code(&state.pool, code).await {
                Ok(Some(a)) => a,
                Ok(None) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "code is invalid or expired"),
                Err(e) => return internal(e),
            };
            // RFC 8707: a resource named at the token endpoint must be the one the owner
            // approved.
            let bad = approval.client_id != client.id
                || form.redirect_uri.as_deref().is_some_and(|u| u != approval.redirect_uri)
                || form.resource.as_deref().is_some_and(|r| {
                    approval.resource.as_deref().is_some_and(|a| a.trim_end_matches('/') != r.trim_end_matches('/'))
                })
                || !pkce_ok(verifier, &approval.code_challenge);
            if bad {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "code does not match this request");
            }
            let grant = match otw_store::mcp_oauth::create_grant(&state.pool, &approval).await {
                Ok(g) => g,
                Err(e) => return internal(e),
            };
            match otw_store::mcp_oauth::issue_tokens(&state.pool, grant.id).await {
                Ok((a, r)) => token_response(a, r),
                Err(e) => internal(e),
            }
        }
        "refresh_token" => {
            let Some(refresh) = &form.refresh_token else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "refresh_token is required");
            };
            use otw_store::mcp_oauth::Refresh;
            match otw_store::mcp_oauth::rotate_refresh(&state.pool, refresh, &client.id).await {
                Ok(Refresh::Rotated { grant_id }) => {
                    match otw_store::mcp_oauth::issue_tokens(&state.pool, grant_id).await {
                        Ok((a, r)) => token_response(a, r),
                        Err(e) => internal(e),
                    }
                }
                Ok(Refresh::Replayed { grant_name }) => {
                    security_events::alert(
                        &state,
                        security_events::MCP_OAUTH_REPLAY,
                        &format!("{}:{}", security_events::MCP_OAUTH_REPLAY, client.id),
                        "An OAuth connection was revoked after a token replay",
                        &format!(
                            "Connection: {grant_name}\nClient: {}\n\nA refresh token that had \
                             already been used was presented again, so a copy of it exists \
                             outside the client. The connection was revoked; reconnect from the \
                             client if it was yours.",
                            client.name
                        ),
                    );
                    oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "refresh token was already used; the grant is revoked")
                }
                Ok(Refresh::Invalid) => {
                    oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "refresh token is invalid or expired")
                }
                Err(e) => internal(e),
            }
        }
        _ => oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", "use authorization_code or refresh_token"),
    }
}

async fn revoke(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<TokenForm>) -> Response {
    if !enabled(&state).await {
        return not_found();
    }
    let client = match authenticate_client(&state, &headers, &form).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    // RFC 7009 §2.2: an unknown token is not an error.
    if let Some(t) = form.token.as_deref().or(form.refresh_token.as_deref()) {
        if let Err(e) = otw_store::mcp_oauth::revoke(&state.pool, t, &client.id).await {
            return internal(e);
        }
    }
    no_store(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(pkce_ok(verifier, challenge));
        assert!(!pkce_ok(verifier, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cX"));
        assert!(!pkce_ok("short", challenge));
    }

    #[test]
    fn redirect_uris_are_https_loopback_or_native() {
        assert!(redirect_uri_allowed("https://claude.ai/api/mcp/auth_callback"));
        assert!(redirect_uri_allowed("http://localhost:33418/callback"));
        assert!(redirect_uri_allowed("http://127.0.0.1/cb"));
        assert!(redirect_uri_allowed("cursor://anysphere.cursor-retrieval/oauth/callback"));
        assert!(redirect_uri_allowed("vscode://vscode.github-authentication/did-authenticate"));
        assert!(!redirect_uri_allowed("ms-settings://x"));
        assert!(!redirect_uri_allowed("myapp://cb"));
        assert!(!redirect_uri_allowed("http://evil.example/cb"));
        assert!(!redirect_uri_allowed("javascript:alert(1)"));
        assert!(!redirect_uri_allowed("https://claude.ai/cb#frag"));
        assert!(!redirect_uri_allowed("not a url"));
    }

    #[test]
    fn loopback_redirects_may_change_port_and_nothing_else() {
        let reg = vec!["http://127.0.0.1:5000/callback".to_string(), "https://a.example/cb".to_string()];
        assert!(redirect_uri_matches(&reg, "https://a.example/cb"));
        assert!(redirect_uri_matches(&reg, "http://127.0.0.1:61234/callback"));
        assert!(!redirect_uri_matches(&reg, "http://127.0.0.1:61234/other"));
        assert!(!redirect_uri_matches(&reg, "https://a.example/cb2"));
        assert!(!redirect_uri_matches(&reg, "http://localhost:5000/callback"));
    }

    #[test]
    fn origin_follows_the_proxy_headers() {
        let mut h = HeaderMap::new();
        h.insert("host", "otw.example.com".parse().unwrap());
        assert_eq!(public_origin(&h, false), "http://otw.example.com");
        assert_eq!(public_origin(&h, true), "https://otw.example.com");
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(public_origin(&h, false), "https://otw.example.com");
        h.insert("host", "evil\"host".parse().unwrap());
        assert_eq!(public_origin(&h, false), "https://localhost");
    }

    #[test]
    fn only_the_mcp_endpoint_is_a_resource() {
        assert!(resource_ok("https://otw.example.com/api/mcp"));
        assert!(resource_ok("https://otw.example.com/api/mcp/"));
        assert!(!resource_ok("https://otw.example.com/api/other"));
        assert!(!resource_ok("garbage"));
    }

    #[test]
    fn basic_credentials_are_form_decoded() {
        use base64::Engine;
        let mut h = HeaderMap::new();
        let raw = base64::engine::general_purpose::STANDARD.encode("otw_cli_a%2Bb:s%3Acret");
        h.insert(header::AUTHORIZATION, format!("Basic {raw}").parse().unwrap());
        assert_eq!(basic_credentials(&h), Some(("otw_cli_a+b".into(), "s:cret".into())));
    }

    #[test]
    fn redirect_keeps_the_clients_own_query() {
        let out = redirect_with("https://a.example/cb?x=1", &[("code", "c d"), ("state", "s")]);
        assert_eq!(out, "https://a.example/cb?x=1&code=c+d&state=s");
    }
}
