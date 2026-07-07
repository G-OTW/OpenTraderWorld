//! Document tree storage for the editor module (folders + pages).
//!
//! Single-user: no owner scoping. A document is either a `folder` (container) or a
//! `page` (rich content). The tree is built via `parent_id`.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// A document node (folder or page). For list/tree views, `content` is omitted.
#[derive(Debug, Serialize)]
pub struct DocumentMeta {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: String,
    pub title: String,
    pub position: f64,
    pub icon: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A full document including its rich content (for the editor view).
#[derive(Debug, Serialize)]
pub struct Document {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: String,
    pub title: String,
    pub content: Option<JsonValue>,
    pub position: f64,
    pub icon: Option<String>,
    pub layout: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// List the whole tree (metadata only), ordered for stable rendering.
pub async fn list_tree(pool: &PgPool) -> anyhow::Result<Vec<DocumentMeta>> {
    let rows = sqlx::query_as::<_, (Uuid, Option<Uuid>, String, String, f64, Option<String>, OffsetDateTime)>(
        "SELECT id, parent_id, kind, title, position, icon, updated_at \
         FROM documents ORDER BY parent_id NULLS FIRST, position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing documents")?;

    Ok(rows
        .into_iter()
        .map(|(id, parent_id, kind, title, position, icon, updated_at)| DocumentMeta {
            id,
            parent_id,
            kind,
            title,
            position,
            icon,
            updated_at,
        })
        .collect())
}

/// Fetch one document with its content.
pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Document>> {
    let row = sqlx::query_as::<_, (Uuid, Option<Uuid>, String, String, Option<JsonValue>, f64, Option<String>, String, OffsetDateTime, OffsetDateTime)>(
        "SELECT id, parent_id, kind, title, content, position, icon, layout, created_at, updated_at \
         FROM documents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|(id, parent_id, kind, title, content, position, icon, layout, created_at, updated_at)| Document {
        id,
        parent_id,
        kind,
        title,
        content,
        position,
        icon,
        layout,
        created_at,
        updated_at,
    });
    Ok(row)
}

/// Create a folder or page. Returns the new document's metadata.
pub async fn create(
    pool: &PgPool,
    parent_id: Option<Uuid>,
    kind: &str,
    title: &str,
) -> anyhow::Result<DocumentMeta> {
    let id = Uuid::new_v4();
    // Append to the end of the sibling list.
    let next_pos: (Option<f64>,) = sqlx::query_as(
        "SELECT MAX(position) FROM documents WHERE parent_id IS NOT DISTINCT FROM $1",
    )
    .bind(parent_id)
    .fetch_one(pool)
    .await?;
    let position = next_pos.0.unwrap_or(0.0) + 1.0;

    sqlx::query(
        "INSERT INTO documents (id, parent_id, kind, title, position) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(parent_id)
    .bind(kind)
    .bind(title)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting document")?;

    Ok(DocumentMeta {
        id,
        parent_id,
        kind: kind.to_string(),
        title: title.to_string(),
        position,
        icon: None,
        updated_at: OffsetDateTime::now_utc(),
    })
}

/// Update a document's title and/or content. Touches updated_at.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    content: Option<&JsonValue>,
    layout: Option<&str>,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE documents SET \
            title = COALESCE($2, title), \
            content = COALESCE($3, content), \
            layout = COALESCE($4, layout), \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .bind(content)
    .bind(layout)
    .execute(pool)
    .await
    .context("updating document")?;
    Ok(res.rows_affected() > 0)
}

/// Move a document under a new parent and/or reposition it.
pub async fn mv(
    pool: &PgPool,
    id: Uuid,
    parent_id: Option<Uuid>,
    position: f64,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE documents SET parent_id = $2, position = $3, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(parent_id)
    .bind(position)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Delete a document (cascades to descendants via the FK).
pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
