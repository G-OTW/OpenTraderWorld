//! Persistence for the Portfolio Tracker's operations-ledger import.
//!
//! Two objects, both owned by the pipeline in `otw-core`:
//!   - **mappings** — a saved source-column → operation-field document, matched to a new
//!     file by header fingerprint. Shared by every portfolio: the mapping describes the
//!     *file*, not the book it lands in.
//!   - **batches**  — one row per committed import, against one portfolio; operations
//!     reference it so an import can be reverted whole.
//!
//! De-duplication is **per portfolio**: the same statement may legitimately feed two
//! books, while importing it twice into one has to stay a no-op.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

// ── Mappings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ImportMapping {
    pub id: Uuid,
    pub name: String,
    pub fingerprint: String,
    pub headers: JsonValue,
    pub mapping: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

const MAPPING_COLUMNS: &str =
    "id, name, fingerprint, headers, mapping, created_at, updated_at, last_used_at";

pub async fn list_mappings(pool: &PgPool) -> anyhow::Result<Vec<ImportMapping>> {
    let sql = format!(
        "SELECT {MAPPING_COLUMNS} FROM portfolio_import_mappings \
         ORDER BY COALESCE(last_used_at, updated_at) DESC"
    );
    sqlx::query_as::<_, ImportMapping>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing portfolio import mappings")
}

pub async fn get_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ImportMapping>> {
    let sql = format!("SELECT {MAPPING_COLUMNS} FROM portfolio_import_mappings WHERE id = $1");
    sqlx::query_as::<_, ImportMapping>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching portfolio import mapping")
}

/// Insert a mapping, or replace the one that already carries this name — a mapping is
/// reused by name, so saving over it keeps its id and the imports pointing at it.
pub async fn upsert_mapping(
    pool: &PgPool,
    name: &str,
    fingerprint: &str,
    headers: &JsonValue,
    mapping: &JsonValue,
) -> anyhow::Result<ImportMapping> {
    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM portfolio_import_mappings WHERE name = $1 LIMIT 1")
            .bind(name)
            .fetch_optional(pool)
            .await
            .context("looking up portfolio import mapping")?;

    let id = match existing {
        Some((id,)) => {
            sqlx::query(
                "UPDATE portfolio_import_mappings \
                 SET fingerprint = $2, headers = $3, mapping = $4, updated_at = now() \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(fingerprint)
            .bind(headers)
            .bind(mapping)
            .execute(pool)
            .await
            .context("updating portfolio import mapping")?;
            id
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO portfolio_import_mappings (id, name, fingerprint, headers, mapping) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(name)
            .bind(fingerprint)
            .bind(headers)
            .bind(mapping)
            .execute(pool)
            .await
            .context("inserting portfolio import mapping")?;
            id
        }
    };
    get_mapping(pool, id)
        .await?
        .context("portfolio import mapping vanished after write")
}

#[derive(Debug, Deserialize, Default)]
pub struct MappingPatch {
    pub name: Option<String>,
    pub mapping: Option<JsonValue>,
}

pub async fn update_mapping(pool: &PgPool, id: Uuid, patch: &MappingPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE portfolio_import_mappings SET \
            name = COALESCE($2, name), \
            mapping = COALESCE($3, mapping), \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.mapping.as_ref())
    .execute(pool)
    .await
    .context("updating portfolio import mapping")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM portfolio_import_mappings WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting portfolio import mapping")?;
    Ok(res.rows_affected() > 0)
}

pub async fn touch_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE portfolio_import_mappings SET last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("touching portfolio import mapping")?;
    Ok(())
}

/// Every saved mapping's id, name, fingerprint and normalized header list — what a
/// freshly uploaded file is scored against.
pub async fn mapping_headers(
    pool: &PgPool,
) -> anyhow::Result<Vec<(Uuid, String, String, Vec<String>)>> {
    let rows: Vec<(Uuid, String, String, JsonValue)> =
        sqlx::query_as("SELECT id, name, fingerprint, headers FROM portfolio_import_mappings")
            .fetch_all(pool)
            .await
            .context("listing portfolio import mapping headers")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, fp, headers)| {
            let list = serde_json::from_value::<Vec<String>>(headers).unwrap_or_default();
            (id, name, fp, list)
        })
        .collect())
}

// ── Batches ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ImportBatch {
    pub id: Uuid,
    pub portfolio_id: Option<Uuid>,
    pub mapping_id: Option<Uuid>,
    pub mapping_name: String,
    pub filename: String,
    pub source_rows: i32,
    pub imported: i32,
    pub duplicates: i32,
    pub failed: i32,
    pub assets_created: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Operations still in the portfolio from this batch (drops as they are deleted).
    pub live_operations: i64,
}

const BATCH_SELECT: &str = "SELECT b.id, b.portfolio_id, b.mapping_id, b.mapping_name, b.filename, \
        b.source_rows, b.imported, b.duplicates, b.failed, b.assets_created, b.created_at, \
        (SELECT COUNT(*) FROM portfolio_operations o WHERE o.import_batch_id = b.id) AS live_operations \
 FROM portfolio_import_batches b";

/// The import history of one portfolio, newest first.
pub async fn list_batches(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<ImportBatch>> {
    let sql = format!("{BATCH_SELECT} WHERE b.portfolio_id = $1 ORDER BY b.created_at DESC LIMIT 200");
    sqlx::query_as::<_, ImportBatch>(sqlx::AssertSqlSafe(sql))
        .bind(portfolio_id)
        .fetch_all(pool)
        .await
        .context("listing portfolio import batches")
}

pub async fn add_batch(
    pool: &PgPool,
    portfolio_id: Uuid,
    mapping_id: Option<Uuid>,
    mapping_name: &str,
    filename: &str,
    source_rows: i32,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portfolio_import_batches \
            (id, portfolio_id, mapping_id, mapping_name, filename, source_rows) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(portfolio_id)
    .bind(mapping_id)
    .bind(mapping_name)
    .bind(filename)
    .bind(source_rows)
    .execute(pool)
    .await
    .context("inserting portfolio import batch")?;
    Ok(id)
}

pub async fn finish_batch(
    pool: &PgPool,
    id: Uuid,
    imported: i32,
    duplicates: i32,
    failed: i32,
    assets_created: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE portfolio_import_batches \
         SET imported = $2, duplicates = $3, failed = $4, assets_created = $5 WHERE id = $1",
    )
    .bind(id)
    .bind(imported)
    .bind(duplicates)
    .bind(failed)
    .bind(assets_created)
    .execute(pool)
    .await
    .context("finishing portfolio import batch")?;
    Ok(())
}

/// Undo an import: delete every operation it created, then the batch row. Assets the
/// import created are left alone — they may already hold hand-entered operations, and an
/// empty asset is one click to remove.
pub async fn revert_batch(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<u64>> {
    let mut tx = pool.begin().await.context("revert batch: begin")?;
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM portfolio_import_batches WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .context("revert batch: lookup")?;
    if exists.is_none() {
        return Ok(None);
    }
    let removed = sqlx::query("DELETE FROM portfolio_operations WHERE import_batch_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("revert batch: deleting operations")?
        .rows_affected();
    sqlx::query("DELETE FROM portfolio_import_batches WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("revert batch: deleting batch")?;
    tx.commit().await.context("revert batch: commit")?;
    Ok(Some(removed))
}

/// Drop the batch from the history, keeping its operations. They become ordinary ones:
/// their batch tag goes **and so does their source-row hash** — a hash whose batch is
/// gone would strand them (unrevertible, and counted as duplicates forever after).
pub async fn forget_batch(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await.context("forget batch: begin")?;
    let res = sqlx::query("DELETE FROM portfolio_import_batches WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("forgetting portfolio import batch")?;
    if res.rows_affected() == 0 {
        return Ok(false);
    }
    // The FK already nulled import_batch_id (ON DELETE SET NULL); untag the rest.
    sqlx::query(
        "UPDATE portfolio_operations SET import_row_hash = NULL \
         WHERE import_batch_id IS NULL AND import_row_hash IS NOT NULL",
    )
    .execute(&mut *tx)
    .await
    .context("forget batch: clearing row hashes")?;
    tx.commit().await.context("forget batch: commit")?;
    Ok(true)
}

// ── Imported operations ──────────────────────────────────────────────────────

/// Insert an operation produced by an import. `row_hash` identifies the source row; a
/// hash already present **in the same portfolio** returns `Ok(false)` so the caller can
/// count a duplicate instead of failing the whole import.
///
/// The arbiter index is partial (`WHERE import_row_hash IS NOT NULL`, so hand-entered
/// operations don't collide on NULL), and Postgres only infers a partial index when the
/// same predicate is repeated on the ON CONFLICT clause — hence the WHERE below.
#[allow(clippy::too_many_arguments)]
pub async fn add_imported_operation(
    pool: &PgPool,
    portfolio_id: Uuid,
    asset_id: Option<Uuid>,
    side: &str,
    op_date: Date,
    quantity: f64,
    price: f64,
    fee: f64,
    note: &str,
    currency: Option<&str>,
    batch_id: Uuid,
    row_hash: &str,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "INSERT INTO portfolio_operations \
            (id, asset_id, portfolio_id, side, op_date, quantity, price, fee, note, currency, \
             import_batch_id, import_row_hash) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
         ON CONFLICT (portfolio_id, import_row_hash) WHERE import_row_hash IS NOT NULL \
         DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(asset_id)
    .bind(portfolio_id)
    .bind(side)
    .bind(op_date)
    .bind(quantity)
    .bind(price)
    .bind(fee)
    .bind(note)
    .bind(currency)
    .bind(batch_id)
    .bind(row_hash)
    .execute(pool)
    .await
    .context("inserting imported operation")?;
    Ok(res.rows_affected() > 0)
}

/// Which of these source rows this portfolio already holds — the preview says "already
/// imported" instead of silently skipping them at commit time.
pub async fn existing_hashes(
    pool: &PgPool,
    portfolio_id: Uuid,
    hashes: &[String],
) -> anyhow::Result<std::collections::HashSet<String>> {
    if hashes.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT import_row_hash FROM portfolio_operations \
         WHERE portfolio_id = $1 AND import_row_hash = ANY($2)",
    )
    .bind(portfolio_id)
    .bind(hashes)
    .fetch_all(pool)
    .await
    .context("checking imported row hashes")?;
    Ok(rows.into_iter().map(|(h,)| h).collect())
}
