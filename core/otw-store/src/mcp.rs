//! MCP access tokens — bearer credentials for the `/api/mcp` endpoint.
//!
//! Tokens are high-entropy (256-bit) opaque strings; only their SHA-256 is stored, so
//! lookup is a direct indexed hash match (no per-request KDF needed — the token itself
//! is unguessable, unlike a password). Permissions are a module → level map where the
//! level is `"r"` (read: GET) or `"rw"` (read + mutations).
//!
//! A token may carry an expiry date (`expires_at`, NULL = never). It is enforced by the
//! caller rather than by the lookup query, so an expired token can be told apart from a
//! wrong one and the user gets the message that names the fix.
//!
//! `external` is a separate dial from the permission map: the levels say what a token may
//! touch, the flag says whether it may be reached from outside the app at all — today, by
//! a chat channel binding. It is opt-in per token, never granted by migration, and it
//! widens nothing on its own: an external token still cannot exceed its own levels.

use anyhow::Context;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct McpToken {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub permissions: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    /// When the token stops being accepted; `None` = never.
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    /// May this token back an external control binding (a chat channel driving OTW)?
    /// Never set by anything but an explicit choice in Settings.
    pub external: bool,
}

impl McpToken {
    /// Whether the token's expiry date has passed. Checked at every use, not at listing:
    /// a token that expires mid-session must stop working there and then.
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|at| at <= OffsetDateTime::now_utc())
    }
}

/// SHA-256 hex of a token string.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a new token: returns the row and the plaintext (shown once, never stored).
pub async fn create_token(
    pool: &PgPool,
    name: &str,
    permissions: &serde_json::Value,
    expires_at: Option<OffsetDateTime>,
    external: bool,
) -> anyhow::Result<(McpToken, String)> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let token = format!("otw_mcp_{hex}");
    let prefix: String = token.chars().take(16).collect();

    let row = sqlx::query_as::<_, McpToken>(
        "INSERT INTO mcp_tokens (id, name, token_hash, prefix, permissions, expires_at, external) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(hash_token(&token))
    .bind(&prefix)
    .bind(permissions)
    .bind(expires_at)
    .bind(external)
    .fetch_one(pool)
    .await
    .context("creating mcp token")?;

    Ok((row, token))
}

pub async fn list_tokens(pool: &PgPool) -> anyhow::Result<Vec<McpToken>> {
    sqlx::query_as(
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at, external \
         FROM mcp_tokens ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing mcp tokens")
}

/// Update name, permissions and/or the external flag; `None` leaves the field unchanged.
pub async fn update_token(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    permissions: Option<&serde_json::Value>,
    external: Option<bool>,
) -> anyhow::Result<Option<McpToken>> {
    sqlx::query_as(
        "UPDATE mcp_tokens SET name = COALESCE($2, name), \
         permissions = COALESCE($3, permissions), external = COALESCE($4, external) \
         WHERE id = $1 \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external",
    )
    .bind(id)
    .bind(name)
    .bind(permissions)
    .bind(external)
    .fetch_optional(pool)
    .await
    .context("updating mcp token")
}

/// Set or clear the expiry date. Separate from [`update_token`] because "no expiry" is a
/// real value here: a COALESCE update could never express it.
pub async fn set_expiry(
    pool: &PgPool,
    id: Uuid,
    expires_at: Option<OffsetDateTime>,
) -> anyhow::Result<Option<McpToken>> {
    sqlx::query_as(
        "UPDATE mcp_tokens SET expires_at = $2 WHERE id = $1 \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external",
    )
    .bind(id)
    .bind(expires_at)
    .fetch_optional(pool)
    .await
    .context("setting mcp token expiry")
}

pub async fn delete_token(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM mcp_tokens WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting mcp token")?;
    Ok(res.rows_affected() > 0)
}

/// Fetch a token row by id (for the in-process agent caller — no plaintext involved).
/// Does not touch `last_used_at`; the agent path logs its own usage.
pub async fn get_token(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<McpToken>> {
    sqlx::query_as(
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at, external \
         FROM mcp_tokens WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching mcp token by id")
}

/// Resolve a presented bearer token to its row (by hash), touching `last_used_at`.
///
/// The second half of the answer is whether this was the token's **first** use, which the
/// caller turns into a security event: a credential minted weeks ago and only now being
/// presented is the moment worth telling the owner about. It has to be read here because
/// the same statement overwrites it: the CTE captures the value the row held before the
/// update, which is why this is not two round trips with a race in the middle.
pub async fn find_by_token(pool: &PgPool, token: &str) -> anyhow::Result<Option<(McpToken, bool)>> {
    let row: Option<(
        Uuid,
        String,
        String,
        serde_json::Value,
        OffsetDateTime,
        Option<OffsetDateTime>,
        Option<OffsetDateTime>,
        bool,
        bool,
    )> = sqlx::query_as(
        "WITH before AS (SELECT token_hash, last_used_at FROM mcp_tokens WHERE token_hash = $1) \
         UPDATE mcp_tokens t SET last_used_at = now() FROM before \
         WHERE t.token_hash = before.token_hash \
         RETURNING t.id, t.name, t.prefix, t.permissions, t.created_at, t.last_used_at, \
                   t.expires_at, t.external, before.last_used_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("resolving mcp token")?;
    Ok(row.map(|(id, name, prefix, permissions, created_at, last_used_at, expires_at, external, first_use)| {
        (
            McpToken {
                id,
                name,
                prefix,
                permissions,
                created_at,
                last_used_at,
                expires_at,
                external,
            },
            first_use,
        )
    }))
}
