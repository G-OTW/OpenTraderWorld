//! Persistence for the Trading Journal's trade-book import.
//!
//! Three objects, all owned by the import pipeline in `otw-core`:
//!   - **mappings**  — a saved source-column → trade-field document, matched to a new
//!     file by header fingerprint. Not to be confused with a journal *template*
//!     (`journal_templates`), which is the hand-logging form.
//!   - **batches**   — one row per committed import; trades reference it so an import
//!     can be reverted whole.
//!   - **aliases**   — headers the user re-mapped by hand, learned for next time.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::journal::TradeInput;

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
        "SELECT {MAPPING_COLUMNS} FROM journal_import_mappings \
         ORDER BY COALESCE(last_used_at, updated_at) DESC"
    );
    sqlx::query_as::<_, ImportMapping>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing import mappings")
}

pub async fn get_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ImportMapping>> {
    let sql = format!("SELECT {MAPPING_COLUMNS} FROM journal_import_mappings WHERE id = $1");
    sqlx::query_as::<_, ImportMapping>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching import mapping")
}

/// Insert a mapping, or replace the one that already carries this name — the name is
/// what the user identifies a mapping by, so saving over it keeps its id (and the past
/// imports that point at it) rather than leaving two entries with the same label.
pub async fn upsert_mapping(
    pool: &PgPool,
    name: &str,
    fingerprint: &str,
    headers: &JsonValue,
    mapping: &JsonValue,
) -> anyhow::Result<ImportMapping> {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM journal_import_mappings WHERE name = $1 LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .context("looking up import mapping")?;

    let id = match existing {
        Some((id,)) => {
            sqlx::query(
                "UPDATE journal_import_mappings \
                 SET fingerprint = $2, headers = $3, mapping = $4, updated_at = now() \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(fingerprint)
            .bind(headers)
            .bind(mapping)
            .execute(pool)
            .await
            .context("updating import mapping")?;
            id
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO journal_import_mappings (id, name, fingerprint, headers, mapping) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(name)
            .bind(fingerprint)
            .bind(headers)
            .bind(mapping)
            .execute(pool)
            .await
            .context("inserting import mapping")?;
            id
        }
    };
    get_mapping(pool, id)
        .await?
        .context("import mapping vanished after write")
}

#[derive(Debug, Deserialize, Default)]
pub struct MappingPatch {
    pub name: Option<String>,
    pub mapping: Option<JsonValue>,
}

pub async fn update_mapping(pool: &PgPool, id: Uuid, patch: &MappingPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_import_mappings SET \
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
    .context("updating import mapping")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_import_mappings WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting import mapping")?;
    Ok(res.rows_affected() > 0)
}

pub async fn touch_mapping(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE journal_import_mappings SET last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("touching import mapping")?;
    Ok(())
}

/// Every saved mapping's id, name and normalized header list — the input the detector
/// scores a freshly uploaded file against.
pub async fn mapping_headers(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, String, String, Vec<String>)>> {
    let rows: Vec<(Uuid, String, String, JsonValue)> = sqlx::query_as(
        "SELECT id, name, fingerprint, headers FROM journal_import_mappings",
    )
    .fetch_all(pool)
    .await
    .context("listing import mapping headers")?;
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
    pub mapping_id: Option<Uuid>,
    pub mapping_name: String,
    pub filename: String,
    pub category_id: Option<Uuid>,
    pub shape: String,
    pub source_rows: i32,
    pub imported: i32,
    pub duplicates: i32,
    pub failed: i32,
    /// "file" (a trade book the user dropped) or "broker" (pulled from an account).
    pub source: String,
    pub broker_account_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Trades still in the journal from this batch (drops as trades are deleted).
    pub live_trades: i64,
}

pub async fn list_batches(pool: &PgPool) -> anyhow::Result<Vec<ImportBatch>> {
    sqlx::query_as::<_, ImportBatch>(
        "SELECT b.id, b.mapping_id, b.mapping_name, b.filename, b.category_id, b.shape, \
                b.source_rows, b.imported, b.duplicates, b.failed, b.source, b.broker_account_id, \
                b.created_at, \
                (SELECT COUNT(*) FROM journal_trades t WHERE t.import_batch_id = b.id) AS live_trades \
         FROM journal_import_batches b ORDER BY b.created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .context("listing import batches")
}

#[allow(clippy::too_many_arguments)]
pub async fn add_batch(
    pool: &PgPool,
    mapping_id: Option<Uuid>,
    mapping_name: &str,
    filename: &str,
    category_id: Option<Uuid>,
    shape: &str,
    source_rows: i32,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO journal_import_batches \
            (id, mapping_id, mapping_name, filename, category_id, shape, source_rows) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(mapping_id)
    .bind(mapping_name)
    .bind(filename)
    .bind(category_id)
    .bind(shape)
    .bind(source_rows)
    .execute(pool)
    .await
    .context("inserting import batch")?;
    Ok(id)
}

/// A batch pulled from a broker account rather than from a file. `label` stands in for the
/// filename in the history list (the account name and the window it covered).
pub async fn add_broker_batch(
    pool: &PgPool,
    account_id: Uuid,
    label: &str,
    category_id: Uuid,
    source_rows: i32,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO journal_import_batches \
            (id, mapping_name, filename, category_id, shape, source_rows, source, broker_account_id) \
         VALUES ($1, '', $2, $3, 'executions', $4, 'broker', $5)",
    )
    .bind(id)
    .bind(label)
    .bind(category_id)
    .bind(source_rows)
    .bind(account_id)
    .execute(pool)
    .await
    .context("inserting broker import batch")?;
    Ok(id)
}

pub async fn finish_batch(
    pool: &PgPool,
    id: Uuid,
    imported: i32,
    duplicates: i32,
    failed: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE journal_import_batches SET imported = $2, duplicates = $3, failed = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(imported)
    .bind(duplicates)
    .bind(failed)
    .execute(pool)
    .await
    .context("finishing import batch")?;
    Ok(())
}

/// Revert an import: delete every trade that came from `id`, then the batch row.
/// Returns the number of trades removed, or None when the batch does not exist.
pub async fn revert_batch(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<u64>> {
    let mut tx = pool.begin().await.context("revert batch: begin")?;
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM journal_import_batches WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .context("revert batch: lookup")?;
    if exists.is_none() {
        return Ok(None);
    }
    let removed = sqlx::query("DELETE FROM journal_trades WHERE import_batch_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("revert batch: deleting trades")?
        .rows_affected();
    sqlx::query("DELETE FROM journal_import_batches WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("revert batch: deleting batch")?;
    tx.commit().await.context("revert batch: commit")?;
    Ok(Some(removed))
}

/// Drop the batch from the history, keeping its trades. They become ordinary trades:
/// their batch tag goes (so the import can no longer be reverted in one click) **and so
/// does their source-row hash**.
///
/// Clearing the hash is the whole point. A trade holding a hash whose batch no longer
/// exists is stranded: it can't be reverted, and re-importing the file it came from
/// counts it as a duplicate, so there is no way back to a revertible state. Forgetting
/// has to mean "detach", not "orphan".
pub async fn forget_batch(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await.context("forget batch: begin")?;
    let res = sqlx::query("DELETE FROM journal_import_batches WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("forgetting import batch")?;
    if res.rows_affected() == 0 {
        return Ok(false);
    }
    // The FK already nulled import_batch_id (ON DELETE SET NULL); untag the rest.
    sqlx::query(
        "UPDATE journal_trades SET import_row_hash = NULL \
         WHERE import_batch_id IS NULL AND import_row_hash IS NOT NULL",
    )
    .execute(&mut *tx)
    .await
    .context("forget batch: clearing row hashes")?;
    tx.commit().await.context("forget batch: commit")?;
    Ok(true)
}

// ── Imported trades ──────────────────────────────────────────────────────────

/// Insert a trade produced by an import. `row_hash` identifies the source row(s); a
/// hash already present **in the same category** returns `Ok(false)` so the caller can
/// count a duplicate instead of failing the whole import.
///
/// De-duplication is per category, because a category is the book being kept: the same
/// statement may legitimately feed two books, while importing it twice into one has to
/// stay a no-op.
///
/// The arbiter index is partial (`WHERE import_row_hash IS NOT NULL`, so hand-logged
/// trades don't collide on NULL), and Postgres only infers a partial index when the
/// same predicate is repeated on the ON CONFLICT clause — hence the WHERE below.
pub async fn add_imported_trade(
    pool: &PgPool,
    input: &TradeInput,
    batch_id: Uuid,
    row_hash: &str,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "INSERT INTO journal_trades \
            (id, category_id, template_id, strategy_id, ticker, asset_class, exchange, side, \
             currency, unit_type, fee_schedule_id, entry_at, exit_at, entry_price, exit_price, \
             quantity, fees, leverage, multiplier, signal_name, feedback, images, fields, \
             advanced, cost_basis_method, entries, exits, brackets, import_batch_id, import_row_hash) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
             $21,$22,$23,$24,$25,$26,$27,$28,$29,$30) \
         ON CONFLICT (category_id, import_row_hash) WHERE import_row_hash IS NOT NULL \
         DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(input.category_id)
    .bind(input.template_id)
    .bind(input.strategy_id)
    .bind(&input.ticker)
    .bind(&input.asset_class)
    .bind(input.exchange.as_deref())
    .bind(&input.side)
    .bind(&input.currency)
    .bind(&input.unit_type)
    .bind(input.fee_schedule_id)
    .bind(input.entry_at)
    .bind(input.exit_at)
    .bind(input.entry_price)
    .bind(input.exit_price)
    .bind(input.quantity)
    .bind(input.fees)
    .bind(input.leverage)
    .bind(input.multiplier)
    .bind(input.signal_name.as_deref())
    .bind(input.feedback.as_deref())
    .bind(&input.images)
    .bind(&input.fields)
    .bind(input.advanced)
    .bind(&input.cost_basis_method)
    .bind(crate::journal::legs_json(&input.entries))
    .bind(crate::journal::legs_json(&input.exits))
    .bind(crate::journal::legs_json(&input.brackets))
    .bind(batch_id)
    .bind(row_hash)
    .execute(pool)
    .await
    .context("inserting imported trade")?;
    Ok(res.rows_affected() > 0)
}

/// Which of these source rows are already in the journal, and in which category — the
/// preview says "already imported" instead of silently skipping them at commit time.
///
/// Returns (category, hash) pairs because de-duplication is per category and one import
/// can file its rows into several (a mapped category column).
pub async fn existing_hashes(
    pool: &PgPool,
    hashes: &[String],
) -> anyhow::Result<std::collections::HashSet<(Uuid, String)>> {
    if hashes.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT category_id, import_row_hash FROM journal_trades \
         WHERE import_row_hash = ANY($1)",
    )
    .bind(hashes)
    .fetch_all(pool)
    .await
    .context("checking imported row hashes")?;
    Ok(rows.into_iter().collect())
}

// Learned header aliases are shared with the other modules that import a file and live
// in `crate::imports`, scoped per target set.

// ── Broker sync ──────────────────────────────────────────────────────────────

/// What happened to one trade a broker sync built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    Inserted,
    /// The same position was already pulled and its legs were refreshed (a position that
    /// was open last time and has closed since).
    Updated,
    /// The hash is taken by a trade this sync does not own (a file import, or a position
    /// whose broker batch was forgotten). Left alone.
    Kept,
}

/// Insert a trade a broker sync built, or refresh the one a previous sync of the same
/// position wrote.
///
/// The identity is the position's opening execution, so the second pull of a wider window
/// lands on the same row instead of filing the trade twice. What the refresh touches is
/// only what the broker owns: the legs, the side and the stamps. Everything the user put
/// there (notes, tags, template fields, strategy) survives, which is the whole reason
/// this is not a delete and re-insert.
pub async fn sync_imported_trade(
    pool: &PgPool,
    input: &TradeInput,
    batch_id: Uuid,
    row_hash: &str,
) -> anyhow::Result<SyncOutcome> {
    let inserted: Option<(bool,)> = sqlx::query_as(
        "INSERT INTO journal_trades \
            (id, category_id, template_id, strategy_id, ticker, asset_class, exchange, side, \
             currency, unit_type, fee_schedule_id, entry_at, exit_at, entry_price, exit_price, \
             quantity, fees, leverage, multiplier, signal_name, feedback, images, fields, \
             advanced, cost_basis_method, entries, exits, brackets, import_batch_id, import_row_hash) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
             $21,$22,$23,$24,$25,$26,$27,$28,$29,$30) \
         ON CONFLICT (category_id, import_row_hash) WHERE import_row_hash IS NOT NULL \
         DO UPDATE SET ticker = EXCLUDED.ticker, asset_class = EXCLUDED.asset_class, \
             exchange = EXCLUDED.exchange, side = EXCLUDED.side, currency = EXCLUDED.currency, \
             unit_type = EXCLUDED.unit_type, entry_at = EXCLUDED.entry_at, \
             exit_at = EXCLUDED.exit_at, entry_price = EXCLUDED.entry_price, \
             exit_price = EXCLUDED.exit_price, quantity = EXCLUDED.quantity, \
             fees = EXCLUDED.fees, multiplier = EXCLUDED.multiplier, \
             advanced = EXCLUDED.advanced, entries = EXCLUDED.entries, exits = EXCLUDED.exits, \
             import_batch_id = EXCLUDED.import_batch_id, updated_at = now() \
         WHERE journal_trades.import_batch_id IN \
               (SELECT id FROM journal_import_batches WHERE source = 'broker') \
         RETURNING (xmax = 0)",
    )
    .bind(Uuid::new_v4())
    .bind(input.category_id)
    .bind(input.template_id)
    .bind(input.strategy_id)
    .bind(&input.ticker)
    .bind(&input.asset_class)
    .bind(input.exchange.as_deref())
    .bind(&input.side)
    .bind(&input.currency)
    .bind(&input.unit_type)
    .bind(input.fee_schedule_id)
    .bind(input.entry_at)
    .bind(input.exit_at)
    .bind(input.entry_price)
    .bind(input.exit_price)
    .bind(input.quantity)
    .bind(input.fees)
    .bind(input.leverage)
    .bind(input.multiplier)
    .bind(input.signal_name.as_deref())
    .bind(input.feedback.as_deref())
    .bind(&input.images)
    .bind(&input.fields)
    .bind(input.advanced)
    .bind(&input.cost_basis_method)
    .bind(crate::journal::legs_json(&input.entries))
    .bind(crate::journal::legs_json(&input.exits))
    .bind(crate::journal::legs_json(&input.brackets))
    .bind(batch_id)
    .bind(row_hash)
    .fetch_optional(pool)
    .await
    .context("syncing a broker trade")?;
    Ok(match inserted {
        Some((true,)) => SyncOutcome::Inserted,
        Some((false,)) => SyncOutcome::Updated,
        None => SyncOutcome::Kept,
    })
}

/// The broker sync a journal last ran, so the modal reopens where the user left it.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BrokerSync {
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    /// Instruments the last pull named, for the brokers that must be asked one at a time.
    pub symbols: JsonValue,
    #[serde(with = "time::serde::rfc3339::option")]
    pub period_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub period_to: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_synced_at: Option<OffsetDateTime>,
}

pub async fn get_broker_sync(
    pool: &PgPool,
    category_id: Uuid,
) -> anyhow::Result<Option<BrokerSync>> {
    Ok(sqlx::query_as::<_, BrokerSync>(
        "SELECT category_id, account_id, symbols, period_from, period_to, last_synced_at \
         FROM journal_broker_syncs WHERE category_id = $1",
    )
    .bind(category_id)
    .fetch_optional(pool)
    .await
    .context("reading the journal's last broker sync")?)
}

/// Remember what this journal was last pulled from. Written on a successful commit only:
/// a preview the user abandoned is not what they want to come back to.
pub async fn set_broker_sync(
    pool: &PgPool,
    category_id: Uuid,
    account_id: Uuid,
    symbols: &JsonValue,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_broker_syncs \
            (category_id, account_id, symbols, period_from, period_to, last_synced_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, now(), now()) \
         ON CONFLICT (category_id) DO UPDATE SET account_id = EXCLUDED.account_id, \
             symbols = EXCLUDED.symbols, period_from = EXCLUDED.period_from, \
             period_to = EXCLUDED.period_to, last_synced_at = now(), updated_at = now()",
    )
    .bind(category_id)
    .bind(account_id)
    .bind(symbols)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .context("remembering the journal's broker sync")?;
    Ok(())
}
