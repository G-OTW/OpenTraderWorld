//! Webhook endpoints — inbound URLs that redirect payloads to a module.
//!
//! Same credential model as MCP tokens: the token is high-entropy (256-bit) and lives in
//! the URL path (many alerting senders can only POST to a bare URL, no custom headers),
//! only its SHA-256 is stored, and the plaintext is returned exactly once at creation.
//! Each endpoint carries a `target` (the module its payloads are redirected to) and a
//! short delivery log for debugging.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Delivery-log entries kept per endpoint; older rows are trimmed on insert.
const EVENTS_KEPT: i64 = 50;
/// Stored payload cap (characters) — enough to debug an alert message, never a dump.
const PAYLOAD_STORED_CHARS: usize = 2000;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WebhookEndpoint {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub target: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub received_count: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_received_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WebhookEvent {
    pub id: Uuid,
    pub status: String,
    pub detail: String,
    pub payload: String,
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,
}

const COLUMNS: &str = "id, name, prefix, target, config, enabled, \
                       received_count, last_received_at, created_at";

/// Mint a new endpoint: returns the row and the plaintext token (shown once, never stored).
pub async fn create_endpoint(
    pool: &PgPool,
    name: &str,
    target: &str,
    config: &serde_json::Value,
) -> anyhow::Result<(WebhookEndpoint, String)> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let token = format!("whk_{hex}");
    let prefix: String = token.chars().take(12).collect();

    let sql = format!(
        "INSERT INTO webhook_endpoints (id, name, token_hash, prefix, target, config) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLUMNS}"
    );
    let row = sqlx::query_as::<_, WebhookEndpoint>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(crate::mcp::hash_token(&token))
        .bind(&prefix)
        .bind(target)
        .bind(config)
        .fetch_one(pool)
        .await
        .context("creating webhook endpoint")?;

    Ok((row, token))
}

pub async fn list_endpoints(pool: &PgPool) -> anyhow::Result<Vec<WebhookEndpoint>> {
    let sql = format!("SELECT {COLUMNS} FROM webhook_endpoints ORDER BY created_at DESC");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing webhook endpoints")
}

/// Update name/target/config/enabled; `None` leaves the field unchanged.
pub async fn update_endpoint(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    target: Option<&str>,
    config: Option<&serde_json::Value>,
    enabled: Option<bool>,
) -> anyhow::Result<Option<WebhookEndpoint>> {
    let sql = format!(
        "UPDATE webhook_endpoints SET \
            name = COALESCE($2, name), target = COALESCE($3, target), \
            config = COALESCE($4, config), enabled = COALESCE($5, enabled) \
         WHERE id = $1 RETURNING {COLUMNS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(target)
        .bind(config)
        .bind(enabled)
        .fetch_optional(pool)
        .await
        .context("updating webhook endpoint")
}

pub async fn delete_endpoint(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM webhook_endpoints WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting webhook endpoint")?;
    Ok(res.rows_affected() > 0)
}

/// Resolve a presented URL token to its endpoint (by hash). Does not filter on `enabled`
/// so the caller can log deliveries to a disabled endpoint as ignored.
pub async fn find_by_token(pool: &PgPool, token: &str) -> anyhow::Result<Option<WebhookEndpoint>> {
    let sql = format!("SELECT {COLUMNS} FROM webhook_endpoints WHERE token_hash = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(crate::mcp::hash_token(token))
        .fetch_optional(pool)
        .await
        .context("resolving webhook token")
}

/// Log one delivery: insert an event (payload truncated), bump the endpoint counters, and
/// trim the log to the last [`EVENTS_KEPT`] entries.
pub async fn record_delivery(
    pool: &PgPool,
    endpoint_id: Uuid,
    status: &str,
    detail: &str,
    payload: &str,
) -> anyhow::Result<()> {
    let stored: String = payload.chars().take(PAYLOAD_STORED_CHARS).collect();
    sqlx::query(
        "INSERT INTO webhook_events (id, endpoint_id, status, detail, payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(endpoint_id)
    .bind(status)
    .bind(detail)
    .bind(stored)
    .execute(pool)
    .await
    .context("inserting webhook event")?;

    sqlx::query(
        "UPDATE webhook_endpoints \
         SET received_count = received_count + 1, last_received_at = now() WHERE id = $1",
    )
    .bind(endpoint_id)
    .execute(pool)
    .await
    .context("bumping webhook counters")?;

    sqlx::query(
        "DELETE FROM webhook_events WHERE endpoint_id = $1 AND id NOT IN \
         (SELECT id FROM webhook_events WHERE endpoint_id = $1 \
          ORDER BY received_at DESC LIMIT $2)",
    )
    .bind(endpoint_id)
    .bind(EVENTS_KEPT)
    .execute(pool)
    .await
    .context("trimming webhook events")?;
    Ok(())
}

pub async fn list_events(
    pool: &PgPool,
    endpoint_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<WebhookEvent>> {
    sqlx::query_as(
        "SELECT id, status, detail, payload, received_at FROM webhook_events \
         WHERE endpoint_id = $1 ORDER BY received_at DESC LIMIT $2",
    )
    .bind(endpoint_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing webhook events")
}
