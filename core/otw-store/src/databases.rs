//! Storage for the editor module's databases (typed tables with rows).
//!
//! A database is a `document` with kind = 'database'. Its columns and rows live
//! here; its view config (table/kanban/gallery) lives in `documents.content`.
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct Column {
    pub id: Uuid,
    pub document_id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub options: JsonValue,
    pub position: f64,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub id: Uuid,
    pub document_id: Uuid,
    pub cells: JsonValue,
    pub position: f64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Full database payload: its columns and rows in render order.
#[derive(Debug, Serialize)]
pub struct DatabaseData {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

// ── Reading ──────────────────────────────────────────────────────────────────

pub async fn load(pool: &PgPool, document_id: Uuid) -> anyhow::Result<DatabaseData> {
    let columns = sqlx::query_as::<_, (Uuid, Uuid, String, String, JsonValue, f64)>(
        "SELECT id, document_id, name, type, options, position \
         FROM database_columns WHERE document_id = $1 ORDER BY position, created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await
    .context("loading database columns")?
    .into_iter()
    .map(|(id, document_id, name, kind, options, position)| Column {
        id,
        document_id,
        name,
        kind,
        options,
        position,
    })
    .collect();

    let rows = sqlx::query_as::<_, (Uuid, Uuid, JsonValue, f64, OffsetDateTime)>(
        "SELECT id, document_id, cells, position, updated_at \
         FROM database_rows WHERE document_id = $1 ORDER BY position, created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await
    .context("loading database rows")?
    .into_iter()
    .map(|(id, document_id, cells, position, updated_at)| Row {
        id,
        document_id,
        cells,
        position,
        updated_at,
    })
    .collect();

    Ok(DatabaseData { columns, rows })
}

// ── Columns ──────────────────────────────────────────────────────────────────

pub const COLUMN_TYPES: [&str; 7] = [
    "text",
    "number",
    "select",
    "multi_select",
    "date",
    "checkbox",
    "url",
];

pub async fn add_column(
    pool: &PgPool,
    document_id: Uuid,
    name: &str,
    kind: &str,
    options: &JsonValue,
) -> anyhow::Result<Column> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) =
        sqlx::query_as("SELECT MAX(position) FROM database_columns WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(pool)
            .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;

    sqlx::query(
        "INSERT INTO database_columns (id, document_id, name, type, options, position) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(document_id)
    .bind(name)
    .bind(kind)
    .bind(options)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting database column")?;

    Ok(Column {
        id,
        document_id,
        name: name.to_string(),
        kind: kind.to_string(),
        options: options.clone(),
        position,
    })
}

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct ColumnPatch {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub options: Option<JsonValue>,
    pub position: Option<f64>,
}

pub async fn update_column(
    pool: &PgPool,
    id: Uuid,
    patch: &ColumnPatch,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE database_columns SET \
            name = COALESCE($2, name), \
            type = COALESCE($3, type), \
            options = COALESCE($4, options), \
            position = COALESCE($5, position) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.kind.as_deref())
    .bind(patch.options.as_ref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating database column")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_column(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM database_columns WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Rows ─────────────────────────────────────────────────────────────────────

pub async fn add_row(
    pool: &PgPool,
    document_id: Uuid,
    cells: &JsonValue,
) -> anyhow::Result<Row> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) =
        sqlx::query_as("SELECT MAX(position) FROM database_rows WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(pool)
            .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;

    sqlx::query(
        "INSERT INTO database_rows (id, document_id, cells, position) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(document_id)
    .bind(cells)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting database row")?;

    Ok(Row {
        id,
        document_id,
        cells: cells.clone(),
        position,
        updated_at: OffsetDateTime::now_utc(),
    })
}

/// Replace a row's cells wholesale (the client sends the full cells object).
pub async fn update_row(pool: &PgPool, id: Uuid, cells: &JsonValue) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE database_rows SET cells = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(cells)
    .execute(pool)
    .await
    .context("updating database row")?;
    Ok(res.rows_affected() > 0)
}

/// Reorder a row (used by table reorder and kanban drag between groups, where
/// the group field is also part of `cells`).
pub async fn move_row(pool: &PgPool, id: Uuid, position: f64) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE database_rows SET position = $2 WHERE id = $1")
        .bind(id)
        .bind(position)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_row(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM database_rows WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
