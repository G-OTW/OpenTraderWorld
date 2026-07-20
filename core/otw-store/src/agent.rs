//! Storage for the Agent module — a light, provider-agnostic chat agent.
//!
//! Six tables (`agent_*`). v0 (Phases 0–1) uses providers, a single default agent,
//! conversations, and messages; memories/skills tables exist for Phase 3 but their CRUD
//! lands with that phase. No vendor is privileged anywhere — the active provider + model
//! come entirely from user config.
//!
//! Provider API keys are **write-only**: accepted on insert/update, never returned by the
//! read helpers (the [`Provider`] struct has no key field; [`provider_key`] fetches it only
//! for the outbound call path). At rest they are sealed with the app's [`SecretCipher`]
//! (same XChaCha20-Poly1305 + `OTW_SECRET_KEY` scheme as feed secrets), stored as
//! `enc1:<b64 nonce>:<b64 ciphertext>` in the existing TEXT column. Values without the
//! prefix are legacy plaintext: still readable, re-sealed on the next key update.
//!
//! Single-user: no owner scoping.

use anyhow::Context;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::crypto::SecretCipher;

mod rfc3339 {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

// ── Providers ─────────────────────────────────────────────────────────────────

/// A provider row with the API key omitted (read-safe: never leaks the secret).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Provider {
    pub id: Uuid,
    pub kind: String,
    pub label: String,
    pub base_url: String,
    pub default_model: String,
    pub enabled: bool,
    /// True when a key is stored, without revealing it (drives the "key set" UI badge).
    pub has_key: bool,
    /// Vault item the key resolves from, when plugged from the centralized vault.
    pub api_key_vault_item: Option<Uuid>,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProviderInput {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    /// Write-only. Empty string on update = leave unchanged; a value replaces it.
    #[serde(default)]
    pub api_key: String,
    /// Reference a centralized vault item instead of pasting a key. Takes precedence
    /// over `api_key` when both are present; setting a direct key clears the reference.
    #[serde(default)]
    pub api_key_vault_item: Option<Uuid>,
    #[serde(default)]
    pub default_model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

const PROVIDER_COLS: &str = "id, kind, label, base_url, default_model, enabled, \
     (api_key <> '' OR api_key_vault_item IS NOT NULL) AS has_key, api_key_vault_item, \
     created_at, updated_at";

pub async fn list_providers(pool: &PgPool) -> anyhow::Result<Vec<Provider>> {
    let sql = format!("SELECT {PROVIDER_COLS} FROM agent_providers ORDER BY created_at");
    sqlx::query_as::<_, Provider>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing agent providers")
}

pub async fn get_provider(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Provider>> {
    let sql = format!("SELECT {PROVIDER_COLS} FROM agent_providers WHERE id = $1");
    sqlx::query_as::<_, Provider>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching agent provider")
}

// ── Key sealing ───────────────────────────────────────────────────────────────

/// Marker prefix for sealed keys in the `api_key` TEXT column.
const KEY_ENC_PREFIX: &str = "enc1:";

/// Seal a plaintext key for storage. Empty stays empty (no key / leave unchanged).
fn seal_key(cipher: &SecretCipher, plaintext: &str) -> anyhow::Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let (nonce, ct) = cipher.seal(plaintext)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(format!("{KEY_ENC_PREFIX}{}:{}", b64.encode(nonce), b64.encode(ct)))
}

/// Open a stored key. Values without the prefix are legacy plaintext.
fn open_key(cipher: &SecretCipher, stored: &str) -> anyhow::Result<String> {
    let Some(rest) = stored.strip_prefix(KEY_ENC_PREFIX) else {
        return Ok(stored.to_string());
    };
    let (n, c) = rest.split_once(':').context("malformed sealed key")?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let nonce = b64.decode(n).context("sealed key nonce b64")?;
    let ct = b64.decode(c).context("sealed key ciphertext b64")?;
    cipher.open(&nonce, &ct)
}

pub async fn add_provider(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &ProviderInput,
) -> anyhow::Result<Provider> {
    let id = Uuid::new_v4();
    // A vault reference wins over a pasted key: never keep both.
    let sealed = match input.api_key_vault_item {
        Some(_) => String::new(),
        None => seal_key(cipher, &input.api_key)?,
    };
    sqlx::query(
        "INSERT INTO agent_providers (id, kind, label, base_url, api_key, default_model, enabled, api_key_vault_item) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(&input.kind)
    .bind(&input.label)
    .bind(&input.base_url)
    .bind(sealed)
    .bind(&input.default_model)
    .bind(input.enabled)
    .bind(input.api_key_vault_item)
    .execute(pool)
    .await
    .context("inserting agent provider")?;
    get_provider(pool, id).await?.context("provider vanished after insert")
}

/// Update a provider. An empty `api_key` with no vault reference leaves the stored key
/// unchanged; a vault reference clears any pasted key and vice versa.
pub async fn update_provider(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &ProviderInput,
) -> anyhow::Result<Option<Provider>> {
    let res = sqlx::query(
        "UPDATE agent_providers SET kind = $2, label = $3, base_url = $4, \
         api_key = CASE WHEN $8::uuid IS NOT NULL THEN '' \
                        WHEN $5 = '' THEN api_key ELSE $5 END, \
         api_key_vault_item = CASE WHEN $8::uuid IS NOT NULL THEN $8 \
                                   WHEN $5 = '' THEN api_key_vault_item ELSE NULL END, \
         default_model = $6, enabled = $7, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.kind)
    .bind(&input.label)
    .bind(&input.base_url)
    .bind(seal_key(cipher, &input.api_key)?)
    .bind(&input.default_model)
    .bind(input.enabled)
    .bind(input.api_key_vault_item)
    .execute(pool)
    .await
    .context("updating agent provider")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_provider(pool, id).await
}

pub async fn delete_provider(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM agent_providers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting agent provider")?;
    Ok(res.rows_affected() > 0)
}

/// Repin a provider and every agent using it onto `model`. Demo-only: the sandbox's
/// seeded model can lose its free tier upstream, so boot resolves a live one and writes
/// it here. Agents that overrode the model with a now-retired slug are moved too —
/// in the demo the override is seeded, not user-chosen.
pub async fn repin_demo_model(pool: &PgPool, provider_id: Uuid, model: &str) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("repin: begin")?;
    sqlx::query("UPDATE agent_providers SET default_model = $2, updated_at = now() WHERE id = $1")
        .bind(provider_id)
        .bind(model)
        .execute(&mut *tx)
        .await
        .context("repinning provider default model")?;
    sqlx::query("UPDATE agent_agents SET model = $2 WHERE provider_id = $1 AND model <> ''")
        .bind(provider_id)
        .bind(model)
        .execute(&mut *tx)
        .await
        .context("repinning agent models")?;
    tx.commit().await.context("repin: commit")?;
    Ok(())
}

/// Fetch and unseal the API key for the outbound call path only (never sent to the client).
/// A vault-plugged key resolves through the vault and counts against its vault-wide quota.
pub async fn provider_key(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT api_key, api_key_vault_item FROM agent_providers WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("fetching provider key")?;
    match row {
        None => Ok(None),
        Some((_, Some(item_id))) => Ok(Some(
            crate::vault::open_item(pool, cipher, item_id)
                .await?
                .context("vault item referenced by provider is missing")?,
        )),
        Some((k, None)) => Ok(Some(open_key(cipher, &k)?)),
    }
}

// ── External MCP servers (agent_mcp_servers) ──────────────────────────────────
//
// Remote Streamable-HTTP MCP servers the agent connects OUT to. Auth values are sealed
// with the same cipher as provider keys and are write-only over the API ([`McpServer`]
// exposes only `has_auth`).

/// A server row with the auth value omitted (read-safe).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct McpServer {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub auth_header: String,
    /// True when an auth value is stored, without revealing it.
    pub has_auth: bool,
    /// Vault item the auth value resolves from, when plugged from the centralized vault.
    pub auth_vault_item: Option<Uuid>,
    pub catalog_id: String,
    pub enabled: bool,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize, Default)]
pub struct McpServerInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_auth_header")]
    pub auth_header: String,
    /// Write-only. Empty on update = leave unchanged; a value replaces it.
    #[serde(default)]
    pub auth_value: String,
    /// Reference a centralized vault item instead of pasting the auth value. Takes
    /// precedence over `auth_value`; setting a direct value clears the reference.
    #[serde(default)]
    pub auth_vault_item: Option<Uuid>,
    #[serde(default)]
    pub catalog_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_auth_header() -> String {
    "Authorization".to_string()
}

const MCP_SERVER_COLS: &str = "id, name, url, auth_header, \
    (auth_value <> '' OR auth_vault_item IS NOT NULL) AS has_auth, auth_vault_item, \
    catalog_id, enabled, created_at, updated_at";

pub async fn list_mcp_servers(pool: &PgPool) -> anyhow::Result<Vec<McpServer>> {
    let sql = format!("SELECT {MCP_SERVER_COLS} FROM agent_mcp_servers ORDER BY name");
    sqlx::query_as::<_, McpServer>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing mcp servers")
}

pub async fn get_mcp_server(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<McpServer>> {
    let sql = format!("SELECT {MCP_SERVER_COLS} FROM agent_mcp_servers WHERE id = $1");
    sqlx::query_as::<_, McpServer>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching mcp server")
}

pub async fn add_mcp_server(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &McpServerInput,
) -> anyhow::Result<McpServer> {
    let id = Uuid::new_v4();
    // A vault reference wins over a pasted value: never keep both.
    let sealed = match input.auth_vault_item {
        Some(_) => String::new(),
        None => seal_key(cipher, &input.auth_value)?,
    };
    sqlx::query(
        "INSERT INTO agent_mcp_servers (id, name, url, auth_header, auth_value, catalog_id, enabled, auth_vault_item) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.url)
    .bind(&input.auth_header)
    .bind(sealed)
    .bind(&input.catalog_id)
    .bind(input.enabled)
    .bind(input.auth_vault_item)
    .execute(pool)
    .await
    .context("inserting mcp server")?;
    get_mcp_server(pool, id).await?.context("mcp server vanished after insert")
}

/// Update a server. An empty `auth_value` with no vault reference leaves the stored
/// value unchanged; a vault reference clears any pasted value and vice versa.
pub async fn update_mcp_server(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &McpServerInput,
) -> anyhow::Result<Option<McpServer>> {
    let res = sqlx::query(
        "UPDATE agent_mcp_servers SET name = $2, url = $3, auth_header = $4, \
         auth_value = CASE WHEN $8::uuid IS NOT NULL THEN '' \
                           WHEN $5 = '' THEN auth_value ELSE $5 END, \
         auth_vault_item = CASE WHEN $8::uuid IS NOT NULL THEN $8 \
                                WHEN $5 = '' THEN auth_vault_item ELSE NULL END, \
         catalog_id = $6, enabled = $7, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.url)
    .bind(&input.auth_header)
    .bind(seal_key(cipher, &input.auth_value)?)
    .bind(&input.catalog_id)
    .bind(input.enabled)
    .bind(input.auth_vault_item)
    .execute(pool)
    .await
    .context("updating mcp server")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_mcp_server(pool, id).await
}

pub async fn delete_mcp_server(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM agent_mcp_servers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting mcp server")?;
    Ok(res.rows_affected() > 0)
}

/// Fetch and unseal a server's auth header + value for the outbound connection only.
/// Returns (header_name, header_value); value is "" when no auth is set. A vault-plugged
/// value resolves through the vault and counts against its vault-wide quota.
pub async fn mcp_server_auth(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
) -> anyhow::Result<Option<(String, String)>> {
    let row: Option<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT auth_header, auth_value, auth_vault_item FROM agent_mcp_servers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching mcp server auth")?;
    match row {
        None => Ok(None),
        Some((h, _, Some(item_id))) => {
            let v = crate::vault::open_item(pool, cipher, item_id)
                .await?
                .context("vault item referenced by mcp server is missing")?;
            Ok(Some((h, v)))
        }
        Some((h, v, None)) => Ok(Some((h, open_key(cipher, &v)?))),
    }
}

// ── Agents ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub system_prompt: String,
    pub provider_id: Option<Uuid>,
    pub model: String,
    pub params: serde_json::Value,
    /// Default MCP token for NEW conversations. The token's per-module permissions
    /// (r/rw/rwd, set in Settings → AI agents) apply directly — no agent-side overlay.
    pub mcp_token_id: Option<Uuid>,
    pub skills: serde_json::Value,
    pub is_default: bool,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

const AGENT_COLS: &str = "id, name, system_prompt, provider_id, model, params, mcp_token_id, \
    skills, is_default, created_at, updated_at";

/// The single default agent, creating it lazily on first use (v0 has exactly one).
pub async fn default_agent(pool: &PgPool) -> anyhow::Result<Agent> {
    let sql = format!("SELECT {AGENT_COLS} FROM agent_agents WHERE is_default LIMIT 1");
    if let Some(agent) = sqlx::query_as::<_, Agent>(sqlx::AssertSqlSafe(sql.clone()))
        .fetch_optional(pool)
        .await
        .context("fetching default agent")?
    {
        return Ok(agent);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_agents (id, name, system_prompt, is_default) \
         VALUES ($1, 'Assistant', $2, TRUE) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(DEFAULT_SYSTEM_PROMPT)
    .execute(pool)
    .await
    .context("seeding default agent")?;
    sqlx::query_as::<_, Agent>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .context("fetching default agent after seed")
}

pub const DEFAULT_SYSTEM_PROMPT: &str =
    "You are a helpful assistant inside OpenTraderWorld, a self-hosted platform for traders. \
     Be concise and accurate. When you are unsure, say so.";

/// Deserialize a present field (including an explicit `null`) as `Some(...)`. With a plain
/// derive, serde flattens both "absent" and `null` to outer `None`, which made clearing
/// provider_id / mcp_token_id impossible: the UI sends `null` to clear.
pub fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize, Default)]
pub struct AgentUpdate {
    pub name: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub provider_id: Option<Option<Uuid>>,
    pub model: Option<String>,
    pub params: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "double_option")]
    pub mcp_token_id: Option<Option<Uuid>>,
    pub skills: Option<serde_json::Value>,
}

pub async fn update_agent(
    pool: &PgPool,
    id: Uuid,
    upd: &AgentUpdate,
) -> anyhow::Result<Option<Agent>> {
    // COALESCE each field so `None` leaves it unchanged. provider_id / mcp_token_id are
    // Option<Option<_>>: outer None = untouched, inner None = explicitly cleared.
    let res = sqlx::query(
        "UPDATE agent_agents SET \
         name = COALESCE($2, name), \
         system_prompt = COALESCE($3, system_prompt), \
         provider_id = CASE WHEN $4 THEN $5 ELSE provider_id END, \
         model = COALESCE($6, model), \
         params = COALESCE($7, params), \
         mcp_token_id = CASE WHEN $8 THEN $9 ELSE mcp_token_id END, \
         skills = COALESCE($10, skills), \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(upd.name.as_deref())
    .bind(upd.system_prompt.as_deref())
    .bind(upd.provider_id.is_some())
    .bind(upd.provider_id.flatten())
    .bind(upd.model.as_deref())
    .bind(upd.params.as_ref())
    .bind(upd.mcp_token_id.is_some())
    .bind(upd.mcp_token_id.flatten())
    .bind(upd.skills.as_ref())
    .execute(pool)
    .await
    .context("updating agent")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    let sql = format!("SELECT {AGENT_COLS} FROM agent_agents WHERE id = $1");
    sqlx::query_as::<_, Agent>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching agent after update")
}

pub async fn get_agent(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Agent>> {
    let sql = format!("SELECT {AGENT_COLS} FROM agent_agents WHERE id = $1");
    sqlx::query_as::<_, Agent>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching agent")
}

// ── Conversations ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub title: String,
    pub summary: String,
    /// How many of the oldest messages are already folded into `summary` (watermark).
    pub summary_covers: i32,
    /// This conversation's tools envelope (NULL = chat only). Prefilled at creation from
    /// the agent's default token, then switchable per conversation.
    pub mcp_token_id: Option<Uuid>,
    /// External MCP servers enabled for this conversation (JSONB array of server UUIDs;
    /// stale ids from deleted servers are ignored at run time).
    pub mcp_servers: serde_json::Value,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Conversation {
    /// The conversation's external-server ids, parsed from the JSONB array.
    pub fn server_ids(&self) -> Vec<Uuid> {
        self.mcp_servers
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().and_then(|s| s.parse().ok())).collect())
            .unwrap_or_default()
    }
}

const CONV_COLS: &str =
    "id, agent_id, title, summary, summary_covers, mcp_token_id, mcp_servers, created_at, updated_at";

pub async fn list_conversations(pool: &PgPool) -> anyhow::Result<Vec<Conversation>> {
    let sql = format!("SELECT {CONV_COLS} FROM agent_conversations ORDER BY updated_at DESC");
    sqlx::query_as::<_, Conversation>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing conversations")
}

pub async fn get_conversation(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Conversation>> {
    let sql = format!("SELECT {CONV_COLS} FROM agent_conversations WHERE id = $1");
    sqlx::query_as::<_, Conversation>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching conversation")
}

/// Create a conversation; `mcp_token_id` is the agent's default token, snapshotted here so
/// later default changes don't retroactively rewire existing conversations.
pub async fn create_conversation(
    pool: &PgPool,
    agent_id: Uuid,
    mcp_token_id: Option<Uuid>,
) -> anyhow::Result<Conversation> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_conversations (id, agent_id, mcp_token_id) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(agent_id)
        .bind(mcp_token_id)
        .execute(pool)
        .await
        .context("creating conversation")?;
    get_conversation(pool, id).await?.context("conversation vanished after insert")
}

/// Replace a conversation's external-server selection.
pub async fn set_conversation_servers(
    pool: &PgPool,
    id: Uuid,
    servers: &[Uuid],
) -> anyhow::Result<bool> {
    let arr = serde_json::json!(servers.iter().map(|u| u.to_string()).collect::<Vec<_>>());
    let res = sqlx::query(
        "UPDATE agent_conversations SET mcp_servers = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(arr)
    .execute(pool)
    .await
    .context("setting conversation servers")?;
    Ok(res.rows_affected() > 0)
}

/// Switch a conversation's tools envelope (None = chat only).
pub async fn set_conversation_token(
    pool: &PgPool,
    id: Uuid,
    token: Option<Uuid>,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE agent_conversations SET mcp_token_id = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(token)
    .execute(pool)
    .await
    .context("setting conversation token")?;
    Ok(res.rows_affected() > 0)
}

pub async fn rename_conversation(pool: &PgPool, id: Uuid, title: &str) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE agent_conversations SET title = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .execute(pool)
    .await
    .context("renaming conversation")?;
    Ok(res.rows_affected() > 0)
}

/// Set the title only if it is still blank (auto-title from the first user message).
pub async fn set_title_if_empty(pool: &PgPool, id: Uuid, title: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE agent_conversations SET title = $2 WHERE id = $1 AND title = ''",
    )
    .bind(id)
    .bind(title)
    .execute(pool)
    .await
    .context("auto-titling conversation")?;
    Ok(())
}

pub async fn touch_conversation(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE agent_conversations SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("touching conversation")?;
    Ok(())
}

pub async fn delete_conversation(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM agent_conversations WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting conversation")?;
    Ok(res.rows_affected() > 0)
}

/// Store the rolling summary of older turns plus the watermark of how many of the oldest
/// messages it now covers.
pub async fn set_summary(
    pool: &PgPool,
    id: Uuid,
    summary: &str,
    covers: i32,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE agent_conversations SET summary = $2, summary_covers = $3 WHERE id = $1")
        .bind(id)
        .bind(summary)
        .bind(covers)
        .execute(pool)
        .await
        .context("setting conversation summary")?;
    Ok(())
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Message {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String,
    pub content: serde_json::Value,
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
}

const MSG_COLS: &str =
    "id, conversation_id, role, content, input_tokens, output_tokens, created_at";

pub async fn list_messages(pool: &PgPool, conversation_id: Uuid) -> anyhow::Result<Vec<Message>> {
    let sql = format!(
        "SELECT {MSG_COLS} FROM agent_messages WHERE conversation_id = $1 ORDER BY created_at"
    );
    sqlx::query_as::<_, Message>(sqlx::AssertSqlSafe(sql))
        .bind(conversation_id)
        .fetch_all(pool)
        .await
        .context("listing messages")
}

/// Append a message; returns the stored row. Also bumps the conversation's updated_at.
pub async fn add_message(
    pool: &PgPool,
    conversation_id: Uuid,
    role: &str,
    content: &serde_json::Value,
    input_tokens: i32,
    output_tokens: i32,
) -> anyhow::Result<Message> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_messages \
         (id, conversation_id, role, content, input_tokens, output_tokens) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(conversation_id)
    .bind(role)
    .bind(content)
    .bind(input_tokens)
    .bind(output_tokens)
    .execute(pool)
    .await
    .context("inserting message")?;
    touch_conversation(pool, conversation_id).await?;
    let sql = format!("SELECT {MSG_COLS} FROM agent_messages WHERE id = $1");
    sqlx::query_as::<_, Message>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .context("fetching message after insert")
}

/// Sum of tokens used across a conversation (for the usage badge).
pub async fn conversation_tokens(pool: &PgPool, conversation_id: Uuid) -> anyhow::Result<(i64, i64)> {
    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT SUM(input_tokens), SUM(output_tokens) FROM agent_messages WHERE conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_one(pool)
    .await
    .context("summing conversation tokens")?;
    Ok((row.0.unwrap_or(0), row.1.unwrap_or(0)))
}

/// Format an OffsetDateTime as RFC3339 (used by the md export).
pub fn rfc3339_string(t: &OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

// ── Long-term memory (agent_memories) ─────────────────────────────────────────
//
// Small md facts the agent writes via `memory_write`. The index (slug + description) is
// injected into the system prompt each run; the full body is fetched with `memory_read`.
// Hard caps keep it light (count + per-memory size), enforced by the API/tool layer.

/// Max stored memories; a new write past this is rejected (the agent must prune first).
pub const MEMORY_MAX_COUNT: i64 = 200;
/// Max characters per memory body.
pub const MEMORY_MAX_LEN: usize = 4000;
/// Max characters per slug. Slugs appear in every system prompt (the memory index).
pub const MEMORY_MAX_SLUG: usize = 64;
/// Max characters per description — ALL descriptions ride in every system prompt, and
/// `memory_write` is model-callable, so an uncapped field is a prompt-bloat vector.
pub const MEMORY_MAX_DESC: usize = 200;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Memory {
    pub id: Uuid,
    pub slug: String,
    pub description: String,
    pub content: String,
    pub kind: String,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

const MEMORY_COLS: &str = "id, slug, description, content, kind, created_at, updated_at";

pub async fn list_memories(pool: &PgPool) -> anyhow::Result<Vec<Memory>> {
    let sql = format!("SELECT {MEMORY_COLS} FROM agent_memories ORDER BY updated_at DESC");
    sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing memories")
}

pub async fn get_memory(pool: &PgPool, slug: &str) -> anyhow::Result<Option<Memory>> {
    let sql = format!("SELECT {MEMORY_COLS} FROM agent_memories WHERE slug = $1");
    sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql))
        .bind(slug)
        .fetch_optional(pool)
        .await
        .context("fetching memory")
}

pub async fn count_memories(pool: &PgPool) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM agent_memories")
        .fetch_one(pool)
        .await
        .context("counting memories")?;
    Ok(row.0)
}

/// The compact index (slug + description) injected into the system prompt.
pub async fn memory_index(pool: &PgPool) -> anyhow::Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT slug, description FROM agent_memories ORDER BY updated_at DESC")
            .fetch_all(pool)
            .await
            .context("loading memory index")?;
    Ok(rows)
}

/// Upsert a memory by slug (create or replace). Returns the stored row.
pub async fn upsert_memory(
    pool: &PgPool,
    slug: &str,
    description: &str,
    content: &str,
    kind: &str,
) -> anyhow::Result<Memory> {
    sqlx::query(
        "INSERT INTO agent_memories (slug, description, content, kind) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (slug) DO UPDATE SET description = $2, content = $3, kind = $4, updated_at = now()",
    )
    .bind(slug)
    .bind(description)
    .bind(content)
    .bind(kind)
    .execute(pool)
    .await
    .context("upserting memory")?;
    get_memory(pool, slug).await?.context("memory vanished after upsert")
}

pub async fn delete_memory(pool: &PgPool, slug: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM agent_memories WHERE slug = $1")
        .bind(slug)
        .execute(pool)
        .await
        .context("deleting memory")?;
    Ok(res.rows_affected() > 0)
}

// ── Skills (agent_skills) ─────────────────────────────────────────────────────
//
// SKILL.md-like md instructions. The catalog (name + description of enabled skills) is
// injected into the system prompt; the full body is loaded on demand via `load_skill`.

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Skill {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub body: String,
    pub enabled: bool,
    pub builtin: bool,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

const SKILL_COLS: &str = "id, name, description, body, enabled, builtin, created_at, updated_at";

pub async fn list_skills(pool: &PgPool) -> anyhow::Result<Vec<Skill>> {
    let sql = format!("SELECT {SKILL_COLS} FROM agent_skills ORDER BY name");
    sqlx::query_as::<_, Skill>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing skills")
}

pub async fn get_skill(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Skill>> {
    let sql = format!("SELECT {SKILL_COLS} FROM agent_skills WHERE id = $1");
    sqlx::query_as::<_, Skill>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching skill")
}

/// Fetch an enabled skill's body by name (for `load_skill`).
pub async fn skill_body(pool: &PgPool, name: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT body FROM agent_skills WHERE name = $1 AND enabled")
            .bind(name)
            .fetch_optional(pool)
            .await
            .context("fetching skill body")?;
    Ok(row.map(|(b,)| b))
}

/// The enabled-skill catalog (name + description) injected into the system prompt.
pub async fn skill_catalog(pool: &PgPool) -> anyhow::Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, description FROM agent_skills WHERE enabled ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .context("loading skill catalog")?;
    Ok(rows)
}

#[derive(Debug, Deserialize, Default)]
pub struct SkillInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

pub async fn add_skill(pool: &PgPool, input: &SkillInput) -> anyhow::Result<Skill> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_skills (id, name, description, body, enabled) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.body)
    .bind(input.enabled)
    .execute(pool)
    .await
    .context("inserting skill")?;
    get_skill(pool, id).await?.context("skill vanished after insert")
}

pub async fn update_skill(
    pool: &PgPool,
    id: Uuid,
    input: &SkillInput,
) -> anyhow::Result<Option<Skill>> {
    let res = sqlx::query(
        "UPDATE agent_skills SET name = $2, description = $3, body = $4, enabled = $5, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.body)
    .bind(input.enabled)
    .execute(pool)
    .await
    .context("updating skill")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_skill(pool, id).await
}

pub async fn delete_skill(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM agent_skills WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting skill")?;
    Ok(res.rows_affected() > 0)
}
