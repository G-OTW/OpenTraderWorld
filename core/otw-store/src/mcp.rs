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
    /// The OAuth client this grant was approved for; `None` for a token minted in Settings.
    pub oauth_client_id: Option<String>,
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
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id",
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
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id \
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
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id",
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
         RETURNING id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id",
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
        "SELECT id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id \
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
        Option<String>,
        bool,
    )> = sqlx::query_as(
        "WITH before AS (SELECT token_hash, last_used_at FROM mcp_tokens WHERE token_hash = $1) \
         UPDATE mcp_tokens t SET last_used_at = now() FROM before \
         WHERE t.token_hash = before.token_hash \
         RETURNING t.id, t.name, t.prefix, t.permissions, t.created_at, t.last_used_at, \
                   t.expires_at, t.external, t.oauth_client_id, before.last_used_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("resolving mcp token")?;
    Ok(row.map(|(id, name, prefix, permissions, created_at, last_used_at, expires_at, external, oauth_client_id, first_use)| {
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
                oauth_client_id,
            },
            first_use,
        )
    }))
}

// ── Call log ─────────────────────────────────────────────────────────────────

/// One gateway tool call, as the `mcp_calls` log records it.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub source: &'static str,
    pub token_id: Option<Uuid>,
    pub token_name: String,
    pub tool: String,
    pub method: String,
    pub route: String,
    pub status: i16,
    pub ok: bool,
    pub class: Option<String>,
    pub detail: Option<String>,
    pub duration_ms: i32,
    pub bytes: i32,
}

/// Retention of the call log.
const CALL_LOG_DAYS: i32 = 30;

pub async fn record_call(pool: &PgPool, r: &CallRecord) -> anyhow::Result<()> {
    static WRITES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    sqlx::query(
        "INSERT INTO mcp_calls (source, token_id, token_name, tool, method, route, status, ok, \
         class, detail, duration_ms, bytes) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(r.source)
    .bind(r.token_id)
    .bind(&r.token_name)
    .bind(&r.tool)
    .bind(&r.method)
    .bind(&r.route)
    .bind(r.status)
    .bind(r.ok)
    .bind(&r.class)
    .bind(&r.detail)
    .bind(r.duration_ms)
    .bind(r.bytes)
    .execute(pool)
    .await
    .context("recording mcp call")?;
    // Prune now and then rather than on a schedule: the log only grows when calls arrive.
    if WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed).is_multiple_of(200) {
        sqlx::query("DELETE FROM mcp_calls WHERE at < now() - make_interval(days => $1)")
            .bind(CALL_LOG_DAYS)
            .execute(pool)
            .await
            .context("pruning mcp call log")?;
    }
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClassCount {
    pub class: String,
    pub calls: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RouteStat {
    pub method: String,
    pub route: String,
    pub calls: i64,
    pub errors: i64,
    pub avg_ms: f64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FailedCall {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub source: String,
    pub token_name: String,
    pub tool: String,
    pub method: String,
    pub route: String,
    pub status: i16,
    pub class: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CallStats {
    pub days: i32,
    pub calls: i64,
    pub errors: i64,
    /// Successful calls that still carry a class (ignored params, schema drift).
    pub notes: i64,
    pub by_class: Vec<ClassCount>,
    pub by_route: Vec<RouteStat>,
    pub recent_failures: Vec<FailedCall>,
}

/// Aggregates over the last `days` days: totals, failures by class, the routes that fail
/// most, and the latest failures with their message.
pub async fn call_stats(pool: &PgPool, days: i32) -> anyhow::Result<CallStats> {
    let window = "at >= now() - make_interval(days => $1)";
    let (calls, errors, notes): (i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE NOT ok), \
                COUNT(*) FILTER (WHERE ok AND class IS NOT NULL) \
         FROM mcp_calls WHERE {window}"
    )))
    .bind(days)
    .fetch_one(pool)
    .await
    .context("mcp call totals")?;
    let by_class = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT class, COUNT(*) AS calls FROM mcp_calls \
         WHERE {window} AND class IS NOT NULL GROUP BY class ORDER BY calls DESC"
    )))
    .bind(days)
    .fetch_all(pool)
    .await
    .context("mcp calls by class")?;
    let by_route = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT method, route, COUNT(*) AS calls, COUNT(*) FILTER (WHERE NOT ok) AS errors, \
                AVG(duration_ms)::float8 AS avg_ms \
         FROM mcp_calls WHERE {window} GROUP BY method, route \
         ORDER BY errors DESC, calls DESC LIMIT 20"
    )))
    .bind(days)
    .fetch_all(pool)
    .await
    .context("mcp calls by route")?;
    let recent_failures = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT at, source, token_name, tool, method, route, status, class, detail \
         FROM mcp_calls WHERE {window} AND NOT ok ORDER BY at DESC LIMIT 30"
    )))
    .bind(days)
    .fetch_all(pool)
    .await
    .context("recent mcp failures")?;
    Ok(CallStats { days, calls, errors, notes, by_class, by_route, recent_failures })
}
