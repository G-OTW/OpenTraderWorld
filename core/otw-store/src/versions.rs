//! Versions of editor documents and saved backtest strategies.
//!
//! A version is a snapshot the user takes on purpose (with an optional note), never an
//! autosave. Versioning is opt-in twice: a global switch per area in `app_settings`, then
//! a `versioned` flag on each document or strategy.
//!
//! Version rows hold the item id without a foreign key. A deleted document takes its
//! history with it; a deleted strategy may leave it behind, listed as "deleted" and
//! restorable or purgeable from there.
//! A database snapshot carries its columns and rows with their original ids: row cells and
//! the view config are keyed by column id, so a restore must give the same ids back.

use anyhow::{bail, Context};
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

/// `app_settings` key of the editor switch ("1" = on).
pub const EDITOR_KEY: &str = "versioning_editor";
/// `app_settings` key of the strategies switch ("1" = on).
pub const STRATEGIES_KEY: &str = "versioning_strategies";

/// Whether the global switch behind `key` is on.
pub async fn enabled(pool: &PgPool, key: &str) -> anyhow::Result<bool> {
    Ok(crate::settings::get(pool, key).await?.as_deref() == Some("1"))
}

/// How much one area's history weighs, for the Settings page.
#[derive(Debug, Serialize)]
pub struct Usage {
    pub versions: i64,
    pub bytes: i64,
}

async fn usage_of(pool: &PgPool, table: &'static str) -> anyhow::Result<Usage> {
    // `table` is one of two fixed identifiers below, never user input.
    let sql = format!("SELECT COUNT(*), pg_total_relation_size('{table}') FROM {table}");
    let (versions, bytes): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .with_context(|| format!("sizing {table}"))?;
    Ok(Usage { versions, bytes })
}

pub async fn editor_usage(pool: &PgPool) -> anyhow::Result<Usage> {
    usage_of(pool, "document_versions").await
}

pub async fn strategies_usage(pool: &PgPool) -> anyhow::Result<Usage> {
    usage_of(pool, "strategy_versions").await
}

/// An item whose history outlived it.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DeletedItem {
    pub id: Uuid,
    /// Name of its latest version.
    pub title: String,
    /// Always `strategy` (documents never outlive their deletion).
    pub kind: String,
    pub versions: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub last_at: OffsetDateTime,
}

// ── Documents ────────────────────────────────────────────────────────────────

/// A document version without its payload, for the history list.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DocVersionMeta {
    pub id: Uuid,
    pub document_id: Uuid,
    pub kind: String,
    pub title: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A full document version.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DocVersion {
    pub id: Uuid,
    pub document_id: Uuid,
    pub kind: String,
    pub title: String,
    pub icon: Option<String>,
    pub layout: String,
    pub content: Option<JsonValue>,
    pub data: Option<JsonValue>,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const DOC_META_COLS: &str = "id, document_id, kind, title, note, created_at";

pub async fn doc_versions(pool: &PgPool, document_id: Uuid) -> anyhow::Result<Vec<DocVersionMeta>> {
    let sql = format!(
        "SELECT {DOC_META_COLS} FROM document_versions WHERE document_id = $1 \
         ORDER BY created_at DESC"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(document_id)
        .fetch_all(pool)
        .await
        .context("listing document versions")?)
}

pub async fn get_doc_version(
    pool: &PgPool,
    document_id: Uuid,
    id: Uuid,
) -> anyhow::Result<Option<DocVersion>> {
    Ok(sqlx::query_as(
        "SELECT id, document_id, kind, title, icon, layout, content, data, note, created_at \
         FROM document_versions WHERE id = $1 AND document_id = $2",
    )
    .bind(id)
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .context("reading document version")?)
}

/// The `data` of a snapshot of document `d`: a database's columns and rows, NULL for a page.
const DOC_DATA_EXPR: &str = "CASE WHEN d.kind = 'database' THEN jsonb_build_object( \
      'columns', COALESCE((SELECT jsonb_agg(jsonb_build_object( \
            'id', c.id, 'name', c.name, 'type', c.type, 'options', c.options, \
            'position', c.position) ORDER BY c.position, c.created_at) \
          FROM database_columns c WHERE c.document_id = d.id), '[]'::jsonb), \
      'rows', COALESCE((SELECT jsonb_agg(jsonb_build_object( \
            'id', r.id, 'cells', r.cells, 'position', r.position) \
            ORDER BY r.position, r.created_at) \
          FROM database_rows r WHERE r.document_id = d.id), '[]'::jsonb)) END";

/// The latest version when the document still matches it exactly, `None` once it was
/// edited since (or has no version).
pub async fn current_doc_version(pool: &PgPool, document_id: Uuid) -> anyhow::Result<Option<Uuid>> {
    let sql = format!(
        "SELECT v.id FROM documents d \
         JOIN LATERAL (SELECT * FROM document_versions WHERE document_id = d.id \
                       ORDER BY created_at DESC LIMIT 1) v ON true \
         WHERE d.id = $1 AND v.kind = d.kind AND v.title = d.title \
           AND v.icon IS NOT DISTINCT FROM d.icon AND v.layout = d.layout \
           AND v.content IS NOT DISTINCT FROM d.content \
           AND v.data IS NOT DISTINCT FROM ({DOC_DATA_EXPR})"
    );
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .context("matching the current document version")?)
}

/// Snapshot a page or a database as it stands. `None` when the document does not exist or
/// is a folder.
async fn snapshot_doc_on(
    conn: &mut PgConnection,
    document_id: Uuid,
    note: &str,
) -> anyhow::Result<Option<DocVersionMeta>> {
    let sql = format!(
        "INSERT INTO document_versions (id, document_id, kind, title, icon, layout, content, data, note) \
         SELECT $1, d.id, d.kind, d.title, d.icon, d.layout, d.content, {DOC_DATA_EXPR}, $2 \
         FROM documents d WHERE d.id = $3 AND d.kind IN ('page', 'database') \
         RETURNING {DOC_META_COLS}"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(note)
        .bind(document_id)
        .fetch_optional(&mut *conn)
        .await
        .context("snapshotting document")?)
}

pub async fn snapshot_doc(
    pool: &PgPool,
    document_id: Uuid,
    note: &str,
) -> anyhow::Result<Option<DocVersionMeta>> {
    let mut conn = pool.acquire().await?;
    snapshot_doc_on(&mut conn, document_id, note).await
}

pub async fn delete_doc_version(pool: &PgPool, document_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM document_versions WHERE id = $1 AND document_id = $2")
        .bind(id)
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Drop every version of one document (live or deleted).
pub async fn purge_doc_versions(pool: &PgPool, document_id: Uuid) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM document_versions WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Turn versioning on or off for one page or database. `false` when no such document
/// (folders are not versioned).
pub async fn set_doc_versioned(pool: &PgPool, id: Uuid, on: bool) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE documents SET versioned = $2 WHERE id = $1 AND kind IN ('page', 'database')",
    )
    .bind(id)
    .bind(on)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Delete a document, its descendants and all their versions.
pub async fn delete_doc(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "WITH RECURSIVE sub AS ( \
            SELECT id FROM documents WHERE id = $1 \
            UNION ALL SELECT d.id FROM documents d JOIN sub ON d.parent_id = sub.id) \
         DELETE FROM document_versions WHERE document_id IN (SELECT id FROM sub)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .context("dropping document versions")?;
    let res = sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

/// Put a version back, then save the result as a new version under `note`, so the history
/// reads one version per save and the restored state is the current one. Returns `None`
/// when the version or the document does not exist.
pub async fn restore_doc_version(
    pool: &PgPool,
    document_id: Uuid,
    id: Uuid,
    note: &str,
) -> anyhow::Result<Option<()>> {
    let Some(v) = get_doc_version(pool, document_id, id).await? else {
        return Ok(None);
    };
    let mut tx = pool.begin().await?;
    let live: Option<(String,)> =
        sqlx::query_as("SELECT kind FROM documents WHERE id = $1 FOR UPDATE")
            .bind(document_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((kind,)) = live else {
        return Ok(None);
    };
    if kind != v.kind {
        bail!("this version is a {} and the document is now a {kind}", v.kind);
    }
    sqlx::query(
        "UPDATE documents SET title = $2, icon = $3, layout = $4, content = $5, \
                updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(&v.title)
    .bind(&v.icon)
    .bind(&v.layout)
    .bind(&v.content)
    .execute(&mut *tx)
    .await
    .context("restoring document")?;
    if v.kind == "database" {
        let data = v.data.unwrap_or_else(|| serde_json::json!({ "columns": [], "rows": [] }));
        sqlx::query("DELETE FROM database_rows WHERE document_id = $1")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM database_columns WHERE document_id = $1")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO database_columns (id, document_id, name, type, options, position) \
             SELECT (c->>'id')::uuid, $1, COALESCE(c->>'name', ''), COALESCE(c->>'type', 'text'), \
                    COALESCE(c->'options', '{}'::jsonb), COALESCE((c->>'position')::float8, 0) \
             FROM jsonb_array_elements(COALESCE($2->'columns', '[]'::jsonb)) c",
        )
        .bind(document_id)
        .bind(&data)
        .execute(&mut *tx)
        .await
        .context("restoring database columns")?;
        sqlx::query(
            "INSERT INTO database_rows (id, document_id, cells, position) \
             SELECT (r->>'id')::uuid, $1, COALESCE(r->'cells', '{}'::jsonb), \
                    COALESCE((r->>'position')::float8, 0) \
             FROM jsonb_array_elements(COALESCE($2->'rows', '[]'::jsonb)) r",
        )
        .bind(document_id)
        .bind(&data)
        .execute(&mut *tx)
        .await
        .context("restoring database rows")?;
    }
    snapshot_doc_on(&mut tx, document_id, note).await?;
    tx.commit().await?;
    Ok(Some(()))
}

/// Drop the whole editor history and switch every document back to unversioned.
pub async fn purge_all_docs(pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM document_versions").execute(&mut *tx).await?;
    sqlx::query("UPDATE documents SET versioned = false WHERE versioned")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

// ── Strategies ───────────────────────────────────────────────────────────────

/// A strategy version without its settings, for the history list.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StrategyVersionMeta {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub name: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A full strategy version.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StrategyVersion {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub settings: JsonValue,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const STRAT_META_COLS: &str = "id, strategy_id, name, note, created_at";

pub async fn strategy_versions(
    pool: &PgPool,
    strategy_id: Uuid,
) -> anyhow::Result<Vec<StrategyVersionMeta>> {
    let sql = format!(
        "SELECT {STRAT_META_COLS} FROM strategy_versions WHERE strategy_id = $1 \
         ORDER BY created_at DESC"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(strategy_id)
        .fetch_all(pool)
        .await
        .context("listing strategy versions")?)
}

pub async fn get_strategy_version(
    pool: &PgPool,
    strategy_id: Uuid,
    id: Uuid,
) -> anyhow::Result<Option<StrategyVersion>> {
    Ok(sqlx::query_as(
        "SELECT id, strategy_id, name, description, tags, settings, note, created_at \
         FROM strategy_versions WHERE id = $1 AND strategy_id = $2",
    )
    .bind(id)
    .bind(strategy_id)
    .fetch_optional(pool)
    .await
    .context("reading strategy version")?)
}

async fn snapshot_strategy_on(
    conn: &mut PgConnection,
    strategy_id: Uuid,
    note: &str,
) -> anyhow::Result<Option<StrategyVersionMeta>> {
    let sql = format!(
        "INSERT INTO strategy_versions (id, strategy_id, name, description, tags, settings, note) \
         SELECT $1, s.id, s.name, s.description, s.tags, s.settings, $2 \
         FROM backtest_strategies s WHERE s.id = $3 \
         RETURNING {STRAT_META_COLS}"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(note)
        .bind(strategy_id)
        .fetch_optional(&mut *conn)
        .await
        .context("snapshotting strategy")?)
}

/// The latest version when the strategy still matches it exactly, `None` once it was
/// edited since (or has no version).
pub async fn current_strategy_version(
    pool: &PgPool,
    strategy_id: Uuid,
) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT v.id FROM backtest_strategies s \
         JOIN LATERAL (SELECT * FROM strategy_versions WHERE strategy_id = s.id \
                       ORDER BY created_at DESC LIMIT 1) v ON true \
         WHERE s.id = $1 AND v.name = s.name AND v.description = s.description \
           AND v.tags = s.tags AND v.settings = s.settings",
    )
    .bind(strategy_id)
    .fetch_optional(pool)
    .await
    .context("matching the current strategy version")?)
}

/// Snapshot a saved strategy as it stands. `None` when it does not exist.
pub async fn snapshot_strategy(
    pool: &PgPool,
    strategy_id: Uuid,
    note: &str,
) -> anyhow::Result<Option<StrategyVersionMeta>> {
    let mut conn = pool.acquire().await?;
    snapshot_strategy_on(&mut conn, strategy_id, note).await
}

pub async fn delete_strategy_version(
    pool: &PgPool,
    strategy_id: Uuid,
    id: Uuid,
) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM strategy_versions WHERE id = $1 AND strategy_id = $2")
        .bind(id)
        .bind(strategy_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn purge_strategy_versions(pool: &PgPool, strategy_id: Uuid) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM strategy_versions WHERE strategy_id = $1")
        .bind(strategy_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn set_strategy_versioned(pool: &PgPool, id: Uuid, on: bool) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE backtest_strategies SET versioned = $2 WHERE id = $1")
        .bind(id)
        .bind(on)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Delete a saved strategy; with `keep_versions` its history stays and shows as deleted.
pub async fn delete_strategy(pool: &PgPool, id: Uuid, keep_versions: bool) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    if !keep_versions {
        sqlx::query("DELETE FROM strategy_versions WHERE strategy_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let res = sqlx::query("DELETE FROM backtest_strategies WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

pub async fn deleted_strategies(pool: &PgPool) -> anyhow::Result<Vec<DeletedItem>> {
    Ok(sqlx::query_as(
        "SELECT v.strategy_id AS id, \
                (array_agg(v.name ORDER BY v.created_at DESC))[1] AS title, \
                'strategy'::text AS kind, \
                COUNT(*) AS versions, MAX(v.created_at) AS last_at \
         FROM strategy_versions v \
         WHERE NOT EXISTS (SELECT 1 FROM backtest_strategies s WHERE s.id = v.strategy_id) \
         GROUP BY v.strategy_id ORDER BY MAX(v.created_at) DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing deleted strategies")?)
}

/// Put a strategy version back, then save the result as a new version under `note`, like
/// a document. A live strategy keeps its current name (names are unique and the library is
/// browsed by them); a deleted one is recreated under its old id, with a numbered name when
/// another strategy took it meanwhile.
pub async fn restore_strategy_version(
    pool: &PgPool,
    strategy_id: Uuid,
    id: Uuid,
    note: &str,
) -> anyhow::Result<Option<()>> {
    let Some(v) = get_strategy_version(pool, strategy_id, id).await? else {
        return Ok(None);
    };
    let mut tx = pool.begin().await?;
    let live: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM backtest_strategies WHERE id = $1 FOR UPDATE")
            .bind(strategy_id)
            .fetch_optional(&mut *tx)
            .await?;
    if live.is_some() {
        sqlx::query(
            "UPDATE backtest_strategies SET description = $2, tags = $3, settings = $4, \
                    updated_at = now() WHERE id = $1",
        )
        .bind(strategy_id)
        .bind(&v.description)
        .bind(&v.tags)
        .bind(&v.settings)
        .execute(&mut *tx)
        .await
        .context("restoring strategy")?;
    } else {
        let mut name = v.name.clone();
        let mut n = 2;
        loop {
            let (taken,): (bool,) =
                sqlx::query_as("SELECT EXISTS (SELECT 1 FROM backtest_strategies WHERE name = $1)")
                    .bind(&name)
                    .fetch_one(&mut *tx)
                    .await?;
            if !taken {
                break;
            }
            name = format!("{} ({n})", v.name);
            n += 1;
        }
        sqlx::query(
            "INSERT INTO backtest_strategies (id, name, description, tags, settings, versioned) \
             VALUES ($1, $2, $3, $4, $5, true)",
        )
        .bind(strategy_id)
        .bind(&name)
        .bind(&v.description)
        .bind(&v.tags)
        .bind(&v.settings)
        .execute(&mut *tx)
        .await
        .context("recreating strategy")?;
    }
    snapshot_strategy_on(&mut tx, strategy_id, note).await?;
    tx.commit().await?;
    Ok(Some(()))
}

/// Drop the whole strategy history and switch every strategy back to unversioned.
pub async fn purge_all_strategies(pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM strategy_versions").execute(&mut *tx).await?;
    sqlx::query("UPDATE backtest_strategies SET versioned = false WHERE versioned")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
