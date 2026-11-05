//! Metadata storage for uploaded files. The bytes live on disk (managed by the
//! core service); this table records what exists and how to serve it.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct FileMeta {
    pub id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn record(
    pool: &PgPool,
    id: Uuid,
    filename: &str,
    content_type: &str,
    size: i64,
) -> anyhow::Result<FileMeta> {
    sqlx::query(
        "INSERT INTO files (id, filename, content_type, size) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(filename)
    .bind(content_type)
    .bind(size)
    .execute(pool)
    .await
    .context("recording file")?;

    Ok(FileMeta {
        id,
        filename: filename.to_string(),
        content_type: content_type.to_string(),
        size,
        created_at: OffsetDateTime::now_utc(),
    })
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<FileMeta>> {
    let row = sqlx::query_as::<_, (Uuid, String, String, i64, OffsetDateTime)>(
        "SELECT id, filename, content_type, size, created_at FROM files WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|(id, filename, content_type, size, created_at)| FileMeta {
        id,
        filename,
        content_type,
        size,
        created_at,
    });
    Ok(row)
}
