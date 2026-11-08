//! Storage for the Community Docs module.
//!
//! A library of community-authored documents shown in-app and kept available offline.
//! Docs originate from the website and are synced daily or manually; each is keyed by a
//! stable `slug`. The body is trusted HTML curated/sanitized at the source. Single-user:
//! no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// A doc without its (potentially large) body — used for the list/rail.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DocSummary {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub categories: Vec<String>,
    pub language: String,
    pub source_url: String,
    #[serde(with = "time::serde::rfc3339")]
    pub synced_at: OffsetDateTime,
    /// Whether the user has pinned this doc to the favorites pane.
    pub favorited: bool,
}

/// A full doc including its HTML body.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Doc {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub categories: Vec<String>,
    pub language: String,
    pub body: String,
    pub source_url: String,
    #[serde(with = "time::serde::rfc3339")]
    pub synced_at: OffsetDateTime,
    /// Whether the user has pinned this doc to the favorites pane.
    pub favorited: bool,
}

/// Incoming doc from a website sync. Upserted by `slug`.
#[derive(Debug, Deserialize, Default)]
pub struct DocInput {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source_url: String,
}

fn default_language() -> String {
    "en".to_string()
}

pub async fn list_docs(pool: &PgPool) -> anyhow::Result<Vec<DocSummary>> {
    let rows = sqlx::query_as::<_, DocSummary>(
        "SELECT d.id, d.slug, d.title, d.summary, d.categories, d.language, d.source_url, d.synced_at, \
                (f.slug IS NOT NULL) AS favorited \
         FROM community_docs d \
         LEFT JOIN community_docs_favorites f ON f.slug = d.slug \
         ORDER BY d.title",
    )
    .fetch_all(pool)
    .await
    .context("listing community docs")?;
    Ok(rows)
}

/// Favorited docs only, most-recently-pinned first — for the persistent left pane.
pub async fn list_favorites(pool: &PgPool) -> anyhow::Result<Vec<DocSummary>> {
    let rows = sqlx::query_as::<_, DocSummary>(
        "SELECT d.id, d.slug, d.title, d.summary, d.categories, d.language, d.source_url, d.synced_at, \
                true AS favorited \
         FROM community_docs d \
         JOIN community_docs_favorites f ON f.slug = d.slug \
         ORDER BY f.favorited_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing favorite community docs")?;
    Ok(rows)
}

pub async fn get_doc(pool: &PgPool, slug: &str) -> anyhow::Result<Option<Doc>> {
    let row = sqlx::query_as::<_, Doc>(
        "SELECT d.id, d.slug, d.title, d.summary, d.categories, d.language, d.body, d.source_url, d.synced_at, \
                (f.slug IS NOT NULL) AS favorited \
         FROM community_docs d \
         LEFT JOIN community_docs_favorites f ON f.slug = d.slug \
         WHERE d.slug = $1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("fetching community doc")?;
    Ok(row)
}

/// Pin or unpin a doc by slug. Returns `false` if no such doc exists.
pub async fn set_favorite(pool: &PgPool, slug: &str, favorite: bool) -> anyhow::Result<bool> {
    if favorite {
        let res = sqlx::query(
            "INSERT INTO community_docs_favorites (slug) \
             SELECT slug FROM community_docs WHERE slug = $1 \
             ON CONFLICT (slug) DO NOTHING",
        )
        .bind(slug)
        .execute(pool)
        .await
        .context("favoriting community doc")?;
        // A missing doc inserts zero rows and no conflict fired, so verify existence.
        if res.rows_affected() == 0 {
            return doc_exists(pool, slug).await;
        }
        Ok(true)
    } else {
        sqlx::query("DELETE FROM community_docs_favorites WHERE slug = $1")
            .bind(slug)
            .execute(pool)
            .await
            .context("unfavoriting community doc")?;
        doc_exists(pool, slug).await
    }
}

async fn doc_exists(pool: &PgPool, slug: &str) -> anyhow::Result<bool> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM community_docs WHERE slug = $1)")
            .bind(slug)
            .fetch_one(pool)
            .await
            .context("checking community doc exists")?;
    Ok(exists)
}

/// Insert or update a doc by `slug` and stamp `synced_at`. Used by the website sync.
pub async fn upsert_doc(pool: &PgPool, input: &DocInput) -> anyhow::Result<Doc> {
    sqlx::query(
        "INSERT INTO community_docs (id, slug, title, summary, categories, language, body, source_url, synced_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now()) \
         ON CONFLICT (slug) DO UPDATE SET \
            title = EXCLUDED.title, summary = EXCLUDED.summary, categories = EXCLUDED.categories, \
            language = EXCLUDED.language, body = EXCLUDED.body, source_url = EXCLUDED.source_url, \
            synced_at = now(), updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(&input.slug)
    .bind(&input.title)
    .bind(&input.summary)
    .bind(&input.categories)
    .bind(&input.language)
    .bind(&input.body)
    .bind(&input.source_url)
    .execute(pool)
    .await
    .context("upserting community doc")?;
    get_doc(pool, &input.slug)
        .await?
        .context("doc vanished after upsert")
}
