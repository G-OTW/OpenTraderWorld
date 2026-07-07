//! Storage for the Resources module.
//!
//! A bookmarks library: user-created master categories, each holding resources with a
//! name, link, and optional description. Single-user: no owner scoping. Display mode and
//! sort direction are a frontend concern and not persisted here.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Resource {
    pub id: Uuid,
    pub category_id: Uuid,
    pub name: String,
    pub link: String,
    pub description: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct CategoryInput {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ResourceInput {
    #[serde(default)]
    pub category_id: Option<Uuid>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub description: String,
}

// ── Categories ─────────────────────────────────────────────────────────────

pub async fn list_categories(pool: &PgPool) -> anyhow::Result<Vec<Category>> {
    let rows = sqlx::query_as::<_, Category>(
        "SELECT id, name FROM resource_categories ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .context("listing resource categories")?;
    Ok(rows)
}

pub async fn add_category(pool: &PgPool, input: &CategoryInput) -> anyhow::Result<Category> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO resource_categories (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(&input.name)
        .execute(pool)
        .await
        .context("inserting resource category")?;
    Ok(Category { id, name: input.name.clone() })
}

pub async fn update_category(
    pool: &PgPool,
    id: Uuid,
    input: &CategoryInput,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE resource_categories SET name = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .execute(pool)
    .await
    .context("updating resource category")?;
    Ok(res.rows_affected() > 0)
}

/// Delete a category and all its resources (ON DELETE CASCADE).
pub async fn delete_category(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM resource_categories WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Resources ──────────────────────────────────────────────────────────────

const COLUMNS: &str = "id, category_id, name, link, description";

pub async fn list_resources(pool: &PgPool) -> anyhow::Result<Vec<Resource>> {
    let sql = format!("SELECT {COLUMNS} FROM resources ORDER BY name");
    let rows = sqlx::query_as::<_, Resource>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing resources")?;
    Ok(rows)
}

pub async fn get_resource(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Resource>> {
    let sql = format!("SELECT {COLUMNS} FROM resources WHERE id = $1");
    let row = sqlx::query_as::<_, Resource>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching resource")?;
    Ok(row)
}

pub async fn add_resource(pool: &PgPool, input: &ResourceInput) -> anyhow::Result<Resource> {
    let id = Uuid::new_v4();
    let category_id = input.category_id.context("category_id required")?;
    sqlx::query(
        "INSERT INTO resources (id, category_id, name, link, description) \
         VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(category_id)
    .bind(&input.name)
    .bind(&input.link)
    .bind(&input.description)
    .execute(pool)
    .await
    .context("inserting resource")?;
    get_resource(pool, id).await?.context("resource vanished after insert")
}

pub async fn update_resource(
    pool: &PgPool,
    id: Uuid,
    input: &ResourceInput,
) -> anyhow::Result<bool> {
    let category_id = input.category_id.context("category_id required")?;
    let res = sqlx::query(
        "UPDATE resources SET category_id = $2, name = $3, link = $4, description = $5, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(category_id)
    .bind(&input.name)
    .bind(&input.link)
    .bind(&input.description)
    .execute(pool)
    .await
    .context("updating resource")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_resource(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM resources WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
