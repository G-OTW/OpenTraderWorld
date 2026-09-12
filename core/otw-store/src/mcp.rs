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
) -> anyhow::Result<(McpToken, String)> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let token = format!("otw_mcp_{hex}");
    let prefix: String = token.chars().take(16).collect();

    let row = sqlx::query_as::<_, McpToken>(
        "INSERT INTO mcp_tokens (id, name, token_hash, prefix, permissions, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(hash_token(&token))
    .bind(&prefix)
    .bind(permissions)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .context("creating mcp token")?;

    Ok((row, token))
}

pub async fn list_tokens(pool: &PgPool) -> anyhow::Result<Vec<McpToken>> {
    sqlx::query_as(
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at \
         FROM mcp_tokens ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing mcp tokens")
}

/// Update name and/or permissions; `None` leaves the field unchanged.
pub async fn update_token(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    permissions: Option<&serde_json::Value>,
) -> anyhow::Result<Option<McpToken>> {
    sqlx::query_as(
        "UPDATE mcp_tokens SET name = COALESCE($2, name), \
         permissions = COALESCE($3, permissions) WHERE id = $1 \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at",
    )
    .bind(id)
    .bind(name)
    .bind(permissions)
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
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at",
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
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at \
         FROM mcp_tokens WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching mcp token by id")
}

/// Resolve a presented bearer token to its row (by hash), touching `last_used_at`.
pub async fn find_by_token(pool: &PgPool, token: &str) -> anyhow::Result<Option<McpToken>> {
    let row: Option<McpToken> = sqlx::query_as(
        "UPDATE mcp_tokens SET last_used_at = now() WHERE token_hash = $1 \
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("resolving mcp token")?;
    Ok(row)
}
