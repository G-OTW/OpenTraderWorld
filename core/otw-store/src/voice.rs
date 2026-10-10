//! Voice control: speech engines (the voice broker), spoken commands and their settings.
//!
//! An engine is where recorded audio goes to become text. The browser's own recognizer is
//! not stored (it runs client side, under the id [`BROWSER_ENGINE`]); server engines are
//! rows here. Engine keys are write-only and sealed exactly like the agent providers' keys,
//! or plugged from the vault.
//!
//! A command maps a phrase, plus alternative phrasings, to an ordered list of steps the
//! browser runs. The steps are opaque JSON to this crate beyond a shape check in the API.
//!
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::agent::{open_key, seal_key};
use crate::crypto::SecretCipher;

/// Engine kinds a row can hold.
pub const ENGINE_KINDS: &[&str] = &["openai_compat", "whisper_cpp"];

/// The client-side recognizer, selectable like an engine but never stored.
pub const BROWSER_ENGINE: &str = "browser";

/// App setting holding [`VoiceSettings`] as JSON.
pub const SETTINGS_KEY: &str = "voice_settings";

// ── Settings ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    /// Master switch: off hides the microphone and ignores the shortcut.
    pub enabled: bool,
    /// `browser`, an engine id, or empty when none is picked yet.
    pub engine: String,
    /// BCP-47 language hint (`en`, `fr-FR`), empty for the engine's own detection.
    pub language: String,
    /// Push-to-talk for commands, `KeyboardEvent.code` with modifiers (`Alt+KeyV`). What is
    /// said becomes a plan, even with a text field focused.
    pub shortcut: String,
    /// Push-to-talk for dictation: what is said is typed into the focused field, never read
    /// as a command.
    pub dictation_shortcut: String,
    /// Hand what no command matches to the agent instead of reporting it.
    pub agent_fallback: bool,
    /// Read the outcome aloud.
    pub speak: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: String::new(),
            language: String::new(),
            shortcut: "Alt+KeyV".into(),
            dictation_shortcut: "Alt+Shift+KeyV".into(),
            agent_fallback: true,
            speak: false,
        }
    }
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<VoiceSettings> {
    let raw = crate::settings::get(pool, SETTINGS_KEY).await?;
    Ok(raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub async fn set_settings(pool: &PgPool, s: &VoiceSettings) -> anyhow::Result<()> {
    crate::settings::set(pool, SETTINGS_KEY, &serde_json::to_string(s)?).await
}

// ── Engines ───────────────────────────────────────────────────────────────────

/// An engine row with the key omitted.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Engine {
    pub id: Uuid,
    pub kind: String,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub api_key_vault_item: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize, Default)]
pub struct EngineInput {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    /// Write-only. Empty on update keeps the stored key.
    #[serde(default)]
    pub api_key: String,
    /// A vault reference wins over a pasted key.
    #[serde(default)]
    pub api_key_vault_item: Option<Uuid>,
    /// True on update to drop the stored key (a local server needs none).
    #[serde(default)]
    pub clear_key: bool,
    #[serde(default)]
    pub model: String,
}

const ENGINE_COLS: &str = "id, kind, label, base_url, model, \
     (api_key <> '' OR api_key_vault_item IS NOT NULL) AS has_key, api_key_vault_item, \
     created_at, updated_at";

pub async fn list_engines(pool: &PgPool) -> anyhow::Result<Vec<Engine>> {
    let sql = format!("SELECT {ENGINE_COLS} FROM voice_engines ORDER BY created_at");
    sqlx::query_as::<_, Engine>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing voice engines")
}

pub async fn get_engine(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Engine>> {
    let sql = format!("SELECT {ENGINE_COLS} FROM voice_engines WHERE id = $1");
    sqlx::query_as::<_, Engine>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching voice engine")
}

pub async fn add_engine(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &EngineInput,
) -> anyhow::Result<Engine> {
    let id = Uuid::new_v4();
    let sealed = match input.api_key_vault_item {
        Some(_) => String::new(),
        None => seal_key(cipher, &input.api_key)?,
    };
    sqlx::query(
        "INSERT INTO voice_engines (id, kind, label, base_url, api_key, api_key_vault_item, model) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(&input.kind)
    .bind(&input.label)
    .bind(&input.base_url)
    .bind(sealed)
    .bind(input.api_key_vault_item)
    .bind(&input.model)
    .execute(pool)
    .await
    .context("inserting voice engine")?;
    get_engine(pool, id).await?.context("voice engine vanished after insert")
}

pub async fn update_engine(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &EngineInput,
) -> anyhow::Result<Option<Engine>> {
    let res = sqlx::query(
        "UPDATE voice_engines SET kind = $2, label = $3, base_url = $4, model = $5, \
         api_key = CASE WHEN $8 THEN '' WHEN $7::uuid IS NOT NULL THEN '' \
                        WHEN $6 = '' THEN api_key ELSE $6 END, \
         api_key_vault_item = CASE WHEN $8 THEN NULL WHEN $7::uuid IS NOT NULL THEN $7 \
                                   WHEN $6 = '' THEN api_key_vault_item ELSE NULL END, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.kind)
    .bind(&input.label)
    .bind(&input.base_url)
    .bind(&input.model)
    .bind(seal_key(cipher, &input.api_key)?)
    .bind(input.api_key_vault_item)
    .bind(input.clear_key)
    .execute(pool)
    .await
    .context("updating voice engine")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_engine(pool, id).await
}

pub async fn delete_engine(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM voice_engines WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting voice engine")?;
    Ok(res.rows_affected() > 0)
}

/// Unsealed key for the outbound call only. Empty when the engine has none.
pub async fn engine_key(pool: &PgPool, cipher: &SecretCipher, id: Uuid) -> anyhow::Result<String> {
    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT api_key, api_key_vault_item FROM voice_engines WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("fetching voice engine key")?;
    match row {
        None => Ok(String::new()),
        Some((_, Some(item))) => crate::vault::open_item(pool, cipher, item)
            .await?
            .context("vault item referenced by the voice engine is missing"),
        Some((k, None)) => open_key(cipher, &k),
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Command {
    pub id: Uuid,
    pub phrase: String,
    pub aliases: Vec<String>,
    pub steps: Value,
    pub bypass_confirm: bool,
    pub enabled: bool,
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CommandInput {
    pub phrase: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub steps: Value,
    #[serde(default)]
    pub bypass_confirm: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

const COMMAND_COLS: &str =
    "id, phrase, aliases, steps, bypass_confirm, enabled, position, created_at, updated_at";

pub async fn list_commands(pool: &PgPool) -> anyhow::Result<Vec<Command>> {
    let sql = format!("SELECT {COMMAND_COLS} FROM voice_commands ORDER BY position, created_at");
    sqlx::query_as::<_, Command>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing voice commands")
}

async fn get_command(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Command>> {
    let sql = format!("SELECT {COMMAND_COLS} FROM voice_commands WHERE id = $1");
    sqlx::query_as::<_, Command>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching voice command")
}

pub async fn add_command(pool: &PgPool, input: &CommandInput) -> anyhow::Result<Command> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO voice_commands (id, phrase, aliases, steps, bypass_confirm, enabled, position) \
         VALUES ($1,$2,$3,$4,$5,$6, \
                 (SELECT COALESCE(MAX(position), -1) + 1 FROM voice_commands))",
    )
    .bind(id)
    .bind(&input.phrase)
    .bind(&input.aliases)
    .bind(&input.steps)
    .bind(input.bypass_confirm)
    .bind(input.enabled)
    .execute(pool)
    .await
    .context("inserting voice command")?;
    get_command(pool, id).await?.context("voice command vanished after insert")
}

pub async fn update_command(
    pool: &PgPool,
    id: Uuid,
    input: &CommandInput,
) -> anyhow::Result<Option<Command>> {
    let res = sqlx::query(
        "UPDATE voice_commands SET phrase = $2, aliases = $3, steps = $4, bypass_confirm = $5, \
         enabled = $6, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.phrase)
    .bind(&input.aliases)
    .bind(&input.steps)
    .bind(input.bypass_confirm)
    .bind(input.enabled)
    .execute(pool)
    .await
    .context("updating voice command")?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_command(pool, id).await
}

pub async fn delete_command(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM voice_commands WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting voice command")?;
    Ok(res.rows_affected() > 0)
}

/// Store a new order: `ids[i]` gets position `i`. Unknown ids are ignored.
pub async fn reorder_commands(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("reorder: begin")?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE voice_commands SET position = $2 WHERE id = $1")
            .bind(id)
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .context("reordering voice commands")?;
    }
    tx.commit().await.context("reorder: commit")?;
    Ok(())
}
