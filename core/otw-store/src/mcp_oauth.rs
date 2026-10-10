//! OAuth 2.1 state for the MCP gateway: registered clients, authorization codes, and the
//! access/refresh tokens hanging off a grant.
//!
//! A grant is an ordinary [`McpToken`] row with `oauth_client_id` set, so permissions,
//! expiry, the call log and revocation all work exactly as for a token minted in Settings.
//! Its own `token_hash` is the hash of a secret nobody ever sees: the grant is only
//! reachable through the short-lived access tokens below. Everything secret is stored as
//! a SHA-256, like the bearer tokens.

use anyhow::Context;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::mcp::{hash_token, McpToken};

/// Access token life. Short, because a leaked one is only stopped by its own clock.
pub const ACCESS_TTL: Duration = Duration::hours(1);
/// Refresh token life, renewed at each rotation: a client idle this long signs in again.
pub const REFRESH_TTL: Duration = Duration::days(30);
/// Authorization code life.
const CODE_TTL: Duration = Duration::minutes(2);
/// A refresh token presented again this soon after its rotation is treated as a client
/// retrying a lost response, not as theft: refused, but the grant survives.
const REUSE_GRACE: Duration = Duration::seconds(30);
/// Clients that never got a grant are dropped after a day: registration is open, so
/// an abandoned or hostile registration must not pile up.
const UNUSED_CLIENT_TTL_HOURS: i32 = 24;
/// Ceiling on registered clients.
pub const MAX_CLIENTS: i64 = 200;

pub const ACCESS_PREFIX: &str = "otw_oat_";
pub const REFRESH_PREFIX: &str = "otw_ort_";

/// A random secret: prefix + 64 hex (256 bits).
pub fn random_secret(prefix: &str) -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{prefix}{hex}"))
}

// ── Clients ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OauthClient {
    pub id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub secret_hash: Option<String>,
}

/// Register a client. Returns its id and, for a confidential client, the secret (shown
/// once). Fails when the ceiling is reached even after dropping unused registrations.
pub async fn register_client(
    pool: &PgPool,
    name: &str,
    redirect_uris: &[String],
    confidential: bool,
) -> anyhow::Result<Option<(String, Option<String>)>> {
    sqlx::query(
        "DELETE FROM mcp_oauth_clients c WHERE c.created_at < now() - make_interval(hours => $1) \
         AND NOT EXISTS (SELECT 1 FROM mcp_tokens t WHERE t.oauth_client_id = c.id)",
    )
    .bind(UNUSED_CLIENT_TTL_HOURS)
    .execute(pool)
    .await
    .context("pruning unused oauth clients")?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_oauth_clients")
        .fetch_one(pool)
        .await
        .context("counting oauth clients")?;
    if count >= MAX_CLIENTS {
        return Ok(None);
    }
    let id = random_secret("otw_cli_")?;
    let secret = if confidential { Some(random_secret("otw_cs_")?) } else { None };
    sqlx::query(
        "INSERT INTO mcp_oauth_clients (id, name, redirect_uris, secret_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(name)
    .bind(redirect_uris)
    .bind(secret.as_deref().map(hash_token))
    .execute(pool)
    .await
    .context("registering oauth client")?;
    Ok(Some((id, secret)))
}

pub async fn get_client(pool: &PgPool, id: &str) -> anyhow::Result<Option<OauthClient>> {
    sqlx::query_as(
        "SELECT id, name, redirect_uris, secret_hash FROM mcp_oauth_clients WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching oauth client")
}

// ── Authorization codes ──────────────────────────────────────────────────────

/// What the owner approved, carried by the code until the client redeems it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Approval {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: Option<String>,
    pub name: String,
    pub permissions: serde_json::Value,
    pub grant_expires_at: Option<OffsetDateTime>,
}

pub async fn create_code(pool: &PgPool, a: &Approval) -> anyhow::Result<String> {
    let code = random_secret("otw_code_")?;
    sqlx::query("DELETE FROM mcp_oauth_codes WHERE expires_at < now()")
        .execute(pool)
        .await
        .context("pruning oauth codes")?;
    sqlx::query(
        "INSERT INTO mcp_oauth_codes (code_hash, client_id, redirect_uri, code_challenge, resource, \
         name, permissions, grant_expires_at, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(hash_token(&code))
    .bind(&a.client_id)
    .bind(&a.redirect_uri)
    .bind(&a.code_challenge)
    .bind(&a.resource)
    .bind(&a.name)
    .bind(&a.permissions)
    .bind(a.grant_expires_at)
    .bind(OffsetDateTime::now_utc() + CODE_TTL)
    .execute(pool)
    .await
    .context("storing oauth code")?;
    Ok(code)
}

/// Redeem a code: it is deleted whatever happens next, so it can never be tried twice.
pub async fn take_code(pool: &PgPool, code: &str) -> anyhow::Result<Option<Approval>> {
    sqlx::query_as(
        "DELETE FROM mcp_oauth_codes WHERE code_hash = $1 AND expires_at > now() \
         RETURNING client_id, redirect_uri, code_challenge, resource, name, permissions, grant_expires_at",
    )
    .bind(hash_token(code))
    .fetch_optional(pool)
    .await
    .context("redeeming oauth code")
}

// ── Grants and tokens ────────────────────────────────────────────────────────

/// Turn a redeemed approval into a grant row.
pub async fn create_grant(pool: &PgPool, a: &Approval) -> anyhow::Result<McpToken> {
    let unreachable = random_secret("otw_grant_")?;
    sqlx::query_as(
        "INSERT INTO mcp_tokens (id, name, token_hash, prefix, permissions, expires_at, external, oauth_client_id) \
         VALUES ($1, $2, $3, $4, $5, $6, false, $7) \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id",
    )
    .bind(Uuid::new_v4())
    .bind(&a.name)
    .bind(hash_token(&unreachable))
    .bind(ACCESS_PREFIX)
    .bind(&a.permissions)
    .bind(a.grant_expires_at)
    .bind(&a.client_id)
    .fetch_one(pool)
    .await
    .context("creating oauth grant")
}

/// Mint a fresh access + refresh pair for a grant.
pub async fn issue_tokens(pool: &PgPool, grant_id: Uuid) -> anyhow::Result<(String, String)> {
    let access = random_secret(ACCESS_PREFIX)?;
    let refresh = random_secret(REFRESH_PREFIX)?;
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "DELETE FROM mcp_oauth_tokens WHERE expires_at < now() - interval '1 day'",
    )
    .execute(pool)
    .await
    .context("pruning oauth tokens")?;
    sqlx::query(
        "INSERT INTO mcp_oauth_tokens (token_hash, grant_id, kind, expires_at) \
         VALUES ($1, $3, 'access', $4), ($2, $3, 'refresh', $5)",
    )
    .bind(hash_token(&access))
    .bind(hash_token(&refresh))
    .bind(grant_id)
    .bind(now + ACCESS_TTL)
    .bind(now + REFRESH_TTL)
    .execute(pool)
    .await
    .context("issuing oauth tokens")?;
    Ok((access, refresh))
}

/// What presenting a refresh token amounted to.
#[derive(Debug, PartialEq)]
pub enum Refresh {
    /// Valid and now spent: issue a new pair for this grant.
    Rotated { grant_id: Uuid },
    /// Already spent long enough ago that this is not a retry: someone else holds a copy.
    /// The grant has been revoked.
    Replayed { grant_name: String },
    /// Unknown, expired, for another client, or a retry inside the grace window.
    Invalid,
}

pub async fn rotate_refresh(pool: &PgPool, token: &str, client_id: &str) -> anyhow::Result<Refresh> {
    type Row = (Uuid, OffsetDateTime, Option<OffsetDateTime>, Option<String>, String, Option<OffsetDateTime>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT o.grant_id, o.expires_at, o.used_at, t.oauth_client_id, t.name, t.expires_at \
         FROM mcp_oauth_tokens o JOIN mcp_tokens t ON t.id = o.grant_id \
         WHERE o.token_hash = $1 AND o.kind = 'refresh'",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("reading refresh token")?;
    let Some((grant_id, expires_at, used_at, owner, grant_name, grant_expires)) = row else {
        return Ok(Refresh::Invalid);
    };
    let now = OffsetDateTime::now_utc();
    // An expired grant answers invalid_grant here, so the client signs in again instead of
    // refreshing into access tokens the gateway would refuse.
    if owner.as_deref() != Some(client_id)
        || expires_at <= now
        || grant_expires.is_some_and(|at| at <= now)
    {
        return Ok(Refresh::Invalid);
    }
    if let Some(used) = used_at {
        if OffsetDateTime::now_utc() - used < REUSE_GRACE {
            return Ok(Refresh::Invalid);
        }
        delete_grant(pool, grant_id).await?;
        return Ok(Refresh::Replayed { grant_name });
    }
    // Conditional on still being unused, so two concurrent refreshes cannot both win.
    let spent = sqlx::query(
        "UPDATE mcp_oauth_tokens SET used_at = now() WHERE token_hash = $1 AND used_at IS NULL",
    )
    .bind(hash_token(token))
    .execute(pool)
    .await
    .context("spending refresh token")?;
    if spent.rows_affected() == 0 {
        return Ok(Refresh::Invalid);
    }
    Ok(Refresh::Rotated { grant_id })
}

/// The access token's own state, so the gateway can tell "expired, refresh it" from
/// "never heard of it".
pub enum Access {
    Valid(McpToken, bool),
    Expired,
    Unknown,
}

/// Resolve an access token to its grant, touching the grant's `last_used_at`. The bool is
/// the grant's first use, as for [`crate::mcp::find_by_token`].
pub async fn find_access(pool: &PgPool, token: &str) -> anyhow::Result<Access> {
    let row: Option<(Uuid, OffsetDateTime)> = sqlx::query_as(
        "SELECT grant_id, expires_at FROM mcp_oauth_tokens WHERE token_hash = $1 AND kind = 'access'",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("resolving oauth access token")?;
    let Some((grant_id, expires_at)) = row else {
        return Ok(Access::Unknown);
    };
    if expires_at <= OffsetDateTime::now_utc() {
        return Ok(Access::Expired);
    }
    let row: Option<(McpToken, bool)> = sqlx::query_as::<_, GrantTouch>(
        "WITH before AS (SELECT id, last_used_at FROM mcp_tokens WHERE id = $1) \
         UPDATE mcp_tokens t SET last_used_at = now() FROM before WHERE t.id = before.id \
         RETURNING t.id, t.name, t.prefix, t.permissions, t.created_at, t.last_used_at, \
                   t.expires_at, t.external, t.oauth_client_id, before.last_used_at IS NULL AS first_use",
    )
    .bind(grant_id)
    .fetch_optional(pool)
    .await
    .context("touching oauth grant")?
    .map(|g| (g.token, g.first_use));
    Ok(match row {
        Some((t, first)) => Access::Valid(t, first),
        None => Access::Unknown,
    })
}

#[derive(sqlx::FromRow)]
struct GrantTouch {
    #[sqlx(flatten)]
    token: McpToken,
    first_use: bool,
}

/// RFC 7009 revocation. A refresh token stands for the whole connection, so revoking it
/// removes the grant (and every token under it); an access token goes on its own.
pub async fn revoke(pool: &PgPool, token: &str, client_id: &str) -> anyhow::Result<()> {
    let row: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT o.grant_id, o.kind, t.oauth_client_id FROM mcp_oauth_tokens o \
         JOIN mcp_tokens t ON t.id = o.grant_id WHERE o.token_hash = $1",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("reading token to revoke")?;
    let Some((grant_id, kind, owner)) = row else {
        return Ok(());
    };
    if owner.as_deref() != Some(client_id) {
        return Ok(());
    }
    if kind == "refresh" {
        delete_grant(pool, grant_id).await?;
    } else {
        sqlx::query("DELETE FROM mcp_oauth_tokens WHERE token_hash = $1")
            .bind(hash_token(token))
            .execute(pool)
            .await
            .context("revoking access token")?;
    }
    Ok(())
}

pub async fn delete_grant(pool: &PgPool, grant_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mcp_tokens WHERE id = $1 AND oauth_client_id IS NOT NULL")
        .bind(grant_id)
        .execute(pool)
        .await
        .context("revoking oauth grant")?;
    Ok(())
}
