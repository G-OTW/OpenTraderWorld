//! Storage for the PromptStore module.
//!
//! A library of saved prompts. Each prompt owns an immutable version history: every
//! content save (create, edit, or rollback) appends a `prompt_store_versions` row and
//! the `prompt_store_prompts` row mirrors the latest version. Rollback re-applies an
//! old version's content as a *new* version, so history is never rewritten.
//!
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Prompt {
    pub id: Uuid,
    pub name: String,
    pub body: String,
    pub tags: Vec<String>,
    /// 1 = thumbs up, -1 = thumbs down, 0 = unrated.
    pub vote: i16,
    /// Latest version number (matches the top `PromptVersion.version`).
    pub version: i32,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Serialize `OffsetDateTime` as an RFC 3339 string (matches other modules).
mod rfc3339 {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PromptVersion {
    pub id: Uuid,
    pub version: i32,
    pub name: String,
    pub body: String,
    pub tags: Vec<String>,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Editable content of a prompt (name/body/tags). Vote is handled separately.
#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct PromptInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Normalise tags: trim, drop empties, dedup (case-insensitive), preserve order.
fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        if seen.iter().any(|s| s.eq_ignore_ascii_case(t)) {
            continue;
        }
        seen.push(t.to_string());
    }
    seen
}

const COLUMNS: &str = "id, name, body, tags, vote, version, created_at, updated_at";

pub async fn list_prompts(pool: &PgPool) -> anyhow::Result<Vec<Prompt>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM prompt_store_prompts ORDER BY updated_at DESC, created_at DESC"
    );
    sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing prompts")
}

pub async fn get_prompt(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Prompt>> {
    let sql = format!("SELECT {COLUMNS} FROM prompt_store_prompts WHERE id = $1");
    sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching prompt")
}

/// Create a prompt and its first version (v1) in one transaction.
pub async fn add_prompt(pool: &PgPool, input: &PromptInput) -> anyhow::Result<Prompt> {
    let id = Uuid::new_v4();
    let tags = clean_tags(&input.tags);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO prompt_store_prompts (id, name, body, tags, version) VALUES ($1,$2,$3,$4,1)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.body)
    .bind(&tags)
    .execute(&mut *tx)
    .await
    .context("inserting prompt")?;
    insert_version(&mut tx, id, 1, &input.name, &input.body, &tags).await?;
    tx.commit().await?;
    get_prompt(pool, id).await?.context("prompt vanished after insert")
}

/// Update content and append a new version. Returns false if the prompt is missing.
pub async fn update_prompt(pool: &PgPool, id: Uuid, input: &PromptInput) -> anyhow::Result<bool> {
    let tags = clean_tags(&input.tags);
    let mut tx = pool.begin().await?;
    let next: Option<(i32,)> = sqlx::query_as(
        "SELECT version + 1 FROM prompt_store_prompts WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .context("locking prompt for update")?;
    let Some((next_version,)) = next else {
        return Ok(false);
    };
    sqlx::query(
        "UPDATE prompt_store_prompts SET name = $2, body = $3, tags = $4, version = $5, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.body)
    .bind(&tags)
    .bind(next_version)
    .execute(&mut *tx)
    .await
    .context("updating prompt")?;
    insert_version(&mut tx, id, next_version, &input.name, &input.body, &tags).await?;
    tx.commit().await?;
    Ok(true)
}

/// List a prompt's version history, newest first.
pub async fn list_versions(pool: &PgPool, prompt_id: Uuid) -> anyhow::Result<Vec<PromptVersion>> {
    sqlx::query_as::<_, PromptVersion>(
        "SELECT id, version, name, body, tags, created_at FROM prompt_store_versions \
         WHERE prompt_id = $1 ORDER BY version DESC",
    )
    .bind(prompt_id)
    .fetch_all(pool)
    .await
    .context("listing prompt versions")
}

/// Roll the prompt back to the content of an earlier version by appending it as a new
/// version (history is preserved). Returns None if the prompt or version is missing.
pub async fn rollback(
    pool: &PgPool,
    prompt_id: Uuid,
    version: i32,
) -> anyhow::Result<Option<Prompt>> {
    let mut tx = pool.begin().await?;
    let cur: Option<(i32,)> = sqlx::query_as(
        "SELECT version FROM prompt_store_prompts WHERE id = $1 FOR UPDATE",
    )
    .bind(prompt_id)
    .fetch_optional(&mut *tx)
    .await
    .context("locking prompt for rollback")?;
    let Some((cur_version,)) = cur else {
        return Ok(None);
    };
    let target: Option<(String, String, Vec<String>)> = sqlx::query_as(
        "SELECT name, body, tags FROM prompt_store_versions WHERE prompt_id = $1 AND version = $2",
    )
    .bind(prompt_id)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .context("fetching rollback target version")?;
    let Some((name, body, tags)) = target else {
        return Ok(None);
    };
    let next_version = cur_version + 1;
    sqlx::query(
        "UPDATE prompt_store_prompts SET name = $2, body = $3, tags = $4, version = $5, \
         updated_at = now() WHERE id = $1",
    )
    .bind(prompt_id)
    .bind(&name)
    .bind(&body)
    .bind(&tags)
    .bind(next_version)
    .execute(&mut *tx)
    .await
    .context("applying rollback")?;
    insert_version(&mut tx, prompt_id, next_version, &name, &body, &tags).await?;
    tx.commit().await?;
    get_prompt(pool, prompt_id).await
}

/// Set the quick vote (thumbs up/down/none). Returns false if the prompt is missing.
pub async fn set_vote(pool: &PgPool, id: Uuid, vote: i16) -> anyhow::Result<bool> {
    let vote = vote.clamp(-1, 1);
    let res = sqlx::query(
        "UPDATE prompt_store_prompts SET vote = $2, updated_at = updated_at WHERE id = $1",
    )
    .bind(id)
    .bind(vote)
    .execute(pool)
    .await
    .context("setting prompt vote")?;
    Ok(res.rows_affected() > 0)
}

/// Duplicate a prompt (content only; fresh history starting at v1, no vote).
pub async fn duplicate(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Prompt>> {
    let Some(src) = get_prompt(pool, id).await? else {
        return Ok(None);
    };
    let copy = PromptInput {
        name: format!("{} (copy)", src.name),
        body: src.body,
        tags: src.tags,
    };
    Ok(Some(add_prompt(pool, &copy).await?))
}

pub async fn delete_prompt(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM prompt_store_prompts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Every distinct tag currently in use, sorted (for the search/filter UI).
pub async fn distinct_tags(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT unnest(tags) AS tag FROM prompt_store_prompts ORDER BY tag",
    )
    .fetch_all(pool)
    .await
    .context("listing distinct tags")?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

async fn insert_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    prompt_id: Uuid,
    version: i32,
    name: &str,
    body: &str,
    tags: &[String],
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO prompt_store_versions (id, prompt_id, version, name, body, tags) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(prompt_id)
    .bind(version)
    .bind(name)
    .bind(body)
    .bind(tags)
    .execute(&mut **tx)
    .await
    .context("inserting prompt version")?;
    Ok(())
}
