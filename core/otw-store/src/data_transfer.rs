//! Move a user's data in and out, one module at a time.
//!
//! This is the *portability* path, deliberately separate from the disaster path. A
//! `pg_dump` (Settings → Backup & restore, Full) copies the whole database, sessions and sealed secrets
//! included, and is restored with core stopped. What lives here instead answers "take my
//! trading journal to another instance" or "load last year's portfolios back": the app is
//! running, the selection is per module, and everything else on the target is left alone.
//!
//! Shape of a bundle (a plain zip, openable by anyone):
//!
//! ```text
//!   manifest.json          what is inside, which app version wrote it
//!   tables/<table>.jsonl   one JSON object per row, in table order
//! ```
//!
//! Modules and their tables come from `data_admin::MODULES`, the same list that sizes and
//! wipes them, so a module added there is exportable with no work here.
//!
//! Two ordering rules matter. `MODULES` lists tables child-first, which is what TRUNCATE
//! CASCADE wants; inserts walk the list *backwards* so a parent row exists before the row
//! that references it. And rows carry their original ids, so every sequence is re-synced
//! after a load or the next insert would collide.

use std::io::Write;

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

/// Bundle layout version. Bumped only when the *shape* changes (file names, manifest
/// fields), not when a module gains a table.
pub const FORMAT_VERSION: u32 = 1;

/// Secret stores, excluded unless the caller explicitly asks for them. These hold values
/// sealed with `OTW_SECRET_KEY`: on a machine with a different key they are unreadable
/// noise, so shipping them by default would move something that cannot work and would put
/// ciphertext in a file people mail around. An explicit list, not a name heuristic, because
/// a heuristic that matches `input_tokens` silently breaks an unrelated table.
const CREDENTIAL_TABLES: &[&str] = &[
    "feed_secrets",
    "histdata_provider_creds",
    "broker_account_creds",
    "vaults",
    "vault_items",
    "notif_channels",
    "agent_providers",
    "webhook_endpoints",
    "mcp_tokens",
];

/// Hard ceiling on a bundle, held in memory while it is built or read. Historical bars are
/// what blow past this, and they are re-downloadable from the provider, so the error names
/// that fix rather than trying to stream gigabytes through the browser.
pub const MAX_BUNDLE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableManifest {
    pub name: String,
    pub rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub id: String,
    pub name: String,
    pub tables: Vec<TableManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// App version that wrote the bundle. A bundle from a newer app is refused on import.
    pub app_version: String,
    pub created_at: String,
    pub includes_credentials: bool,
    pub modules: Vec<ModuleManifest>,
}

/// What a load did, per module, for the report shown to the user.
#[derive(Debug, Clone, Serialize)]
pub struct ImportedModule {
    pub id: String,
    pub name: String,
    pub rows: u64,
    pub replaced: bool,
}

/// How an incoming bundle meets the data already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// Wipe the module's tables, then load. The module ends up as the bundle describes it.
    Replace,
    /// Keep what is there and add what is missing; a row whose id or unique key already
    /// exists is skipped, never overwritten.
    Merge,
}

impl ImportMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "replace" => Some(Self::Replace),
            "merge" => Some(Self::Merge),
            _ => None,
        }
    }
}

/// Build a zip bundle for `module_ids`. Unknown ids and modules that own no table are
/// skipped silently (they are display-only). `credentials` includes the sealed stores.
pub async fn export_bundle(
    pool: &PgPool,
    module_ids: &[String],
    credentials: bool,
    app_version: &str,
) -> anyhow::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut modules = Vec::new();
    for id in module_ids {
        let Some((name, tables)) = crate::data_admin::module_tables(id) else {
            continue;
        };
        let mut table_manifests = Vec::new();
        for &table in tables {
            if !credentials && CREDENTIAL_TABLES.contains(&table) {
                continue;
            }
            if !table_exists(pool, table).await? {
                continue;
            }
            // row_to_json keeps column names and types as Postgres renders them, which is
            // exactly what json_populate_recordset reads back on the way in.
            let sql = format!("SELECT row_to_json(t)::text FROM {table} t");
            let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_all(pool)
                .await
                .with_context(|| format!("exporting table {table}"))?;

            zip.start_file(format!("tables/{table}.jsonl"), opts)?;
            for row in &rows {
                zip.write_all(row.as_bytes())?;
                zip.write_all(b"\n")?;
                let so_far = zip.get_ref().map_or(0, |c| c.get_ref().len());
                if so_far > MAX_BUNDLE_BYTES {
                    bail!(
                        "this selection is too large to download in one file. Untick \
                         Historical data (it can be downloaded again from your provider) or \
                         use the full database backup in Settings → Backup & restore instead."
                    );
                }
            }
            table_manifests.push(TableManifest { name: table.to_string(), rows: rows.len() as u64 });
        }
        modules.push(ModuleManifest {
            id: id.clone(),
            name: name.to_string(),
            tables: table_manifests,
        });
    }

    let manifest = Manifest {
        format: FORMAT_VERSION,
        app_version: app_version.to_string(),
        created_at: now_rfc3339(),
        includes_credentials: credentials,
        modules,
    };
    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    Ok(zip.finish()?.into_inner())
}

/// Read a bundle's manifest without loading anything. Used to show the user what a file
/// contains before they commit to it.
pub fn read_manifest(bundle: &[u8]) -> anyhow::Result<Manifest> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bundle))
        .context("this file is not an OpenTraderWorld data file")?;
    let entry = zip
        .by_name("manifest.json")
        .context("this file is not an OpenTraderWorld data file (no manifest inside)")?;
    serde_json::from_reader(entry).context("the data file's manifest is damaged")
}

/// Above this estimate a table is reported from the planner's statistics instead of being
/// counted row by row: a preview has to come back while the user is still looking at it,
/// and the only tables this big are bars, where a rounded number says the same thing.
const EXACT_COUNT_LIMIT: f32 = 500_000.0;

/// One table, as the file has it against as this instance has it.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewTable {
    pub name: String,
    /// Exact: the manifest counts what was written.
    pub incoming_rows: u64,
    pub here_rows: i64,
    /// False when `here_rows` is an estimate rather than a count.
    pub here_exact: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewModule {
    pub id: String,
    pub name: String,
    pub incoming_rows: u64,
    pub here_rows: i64,
    pub here_exact: bool,
    pub tables: Vec<PreviewTable>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub app_version: String,
    pub created_at: String,
    pub includes_credentials: bool,
    pub modules: Vec<PreviewModule>,
    /// Module ids the file carries that this instance does not know, so they would be
    /// skipped. An older bundle, or one from a build with a module this one lacks.
    pub unknown_modules: Vec<String>,
}

/// What loading a bundle would meet, without loading it: the row counts the file carries
/// (its manifest already holds them, so nothing is unzipped) against what is here now.
///
/// The version checks are the load's own, so a file the load would refuse is refused here
/// too, at the point where it costs the user nothing.
///
/// Replace turns these numbers into a promise: `here_rows` is what it erases and
/// `incoming_rows` is what it puts there. Merge cannot be promised the same way: it skips
/// rows whose key already exists, and only the insert knows how many that is, so
/// `incoming_rows` is a ceiling there, not a count of what will be added.
pub async fn preview_bundle(
    pool: &PgPool,
    bundle: &[u8],
    app_version: &str,
) -> anyhow::Result<Preview> {
    let manifest = read_manifest(bundle)?;
    if manifest.format > FORMAT_VERSION {
        bail!("this data file was written by a newer version of OpenTraderWorld. Update this instance first, then load it again.");
    }
    if is_newer(&manifest.app_version, app_version) {
        bail!(
            "this data file comes from version {} and this instance runs {}. Update this instance first, then load it again.",
            manifest.app_version,
            app_version
        );
    }

    let mut modules = Vec::new();
    let mut unknown_modules = Vec::new();

    for m in &manifest.modules {
        let Some((name, tables)) = crate::data_admin::module_tables(&m.id) else {
            unknown_modules.push(m.id.clone());
            continue;
        };
        // The module's own table order, not the file's: a bundle written by an older
        // release may carry fewer tables, and a table it does not carry still has a count
        // here that Replace would erase.
        let mut rows = Vec::with_capacity(tables.len());
        for &table in tables.iter().rev() {
            let incoming_rows = m
                .tables
                .iter()
                .find(|t| t.name == table)
                .map(|t| t.rows)
                .unwrap_or(0);
            let (here_rows, here_exact) = count_rows(pool, table).await?;
            if incoming_rows == 0 && here_rows == 0 {
                continue; // Empty on both sides: noise in a list meant to be read.
            }
            rows.push(PreviewTable { name: table.to_string(), incoming_rows, here_rows, here_exact });
        }

        modules.push(PreviewModule {
            id: m.id.clone(),
            name: name.to_string(),
            incoming_rows: rows.iter().map(|t| t.incoming_rows).sum(),
            here_rows: rows.iter().map(|t| t.here_rows).sum(),
            here_exact: rows.iter().all(|t| t.here_exact),
            tables: rows,
        });
    }

    Ok(Preview {
        app_version: manifest.app_version,
        created_at: manifest.created_at,
        includes_credentials: manifest.includes_credentials,
        modules,
        unknown_modules,
    })
}

/// Rows in `table` now: counted exactly, unless the table is large enough that counting it
/// would stall the preview, in which case the estimate is returned and flagged as one.
async fn count_rows(pool: &PgPool, table: &str) -> anyhow::Result<(i64, bool)> {
    let estimate: Option<f32> =
        sqlx::query_scalar("SELECT reltuples FROM pg_class WHERE oid = to_regclass($1)")
            .bind(table)
            .fetch_optional(pool)
            .await
            .with_context(|| format!("looking up {table}"))?
            .flatten();
    let Some(estimate) = estimate else {
        return Ok((0, true)); // Table absent from this schema.
    };
    if estimate > EXACT_COUNT_LIMIT {
        return Ok((estimate as i64, false));
    }
    // Identifier from MODULES, never user input.
    let sql = format!("SELECT count(*) FROM {table}");
    let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .with_context(|| format!("counting {table}"))?;
    Ok((n, true))
}

/// Load selected modules from a bundle. Everything happens in one transaction: a failure
/// anywhere leaves the database exactly as it was, which is what makes Replace safe to
/// offer at all.
pub async fn import_bundle(
    pool: &PgPool,
    bundle: &[u8],
    module_ids: &[String],
    mode: ImportMode,
    app_version: &str,
) -> anyhow::Result<Vec<ImportedModule>> {
    let manifest = read_manifest(bundle)?;
    if manifest.format > FORMAT_VERSION {
        bail!("this data file was written by a newer version of OpenTraderWorld. Update this instance first, then load it again.");
    }
    if is_newer(&manifest.app_version, app_version) {
        bail!(
            "this data file comes from version {} and this instance runs {}. Update this instance first, then load it again.",
            manifest.app_version,
            app_version
        );
    }

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bundle))
        .context("this file is not an OpenTraderWorld data file")?;

    let mut tx = pool.begin().await?;
    let mut report = Vec::new();

    for id in module_ids {
        let Some((name, tables)) = crate::data_admin::module_tables(id) else {
            continue;
        };
        if !manifest.modules.iter().any(|m| &m.id == id) {
            continue; // Not in this file; nothing to load, not an error.
        }

        if mode == ImportMode::Replace && !tables.is_empty() {
            // Child-first, which is the order MODULES already declares.
            let list = tables.join(", ");
            let sql = format!("TRUNCATE TABLE {list} RESTART IDENTITY CASCADE");
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&mut *tx)
                .await
                .with_context(|| format!("clearing module {id} before loading"))?;
        }

        let mut rows_loaded: u64 = 0;
        // Parents before children on the way in: the declared order reversed.
        for &table in tables.iter().rev() {
            let Ok(mut entry) = zip.by_name(&format!("tables/{table}.jsonl")) else {
                continue; // Table absent from the bundle (older export, or credentials left out).
            };
            let mut raw = String::new();
            std::io::Read::read_to_string(&mut entry, &mut raw)
                .with_context(|| format!("reading {table} from the data file"))?;
            drop(entry);
            if raw.trim().is_empty() {
                continue;
            }
            if !table_exists_tx(&mut tx, table).await? {
                continue;
            }

            // One statement per batch: json_populate_recordset expands the array into rows
            // of the table's own type, so columns and types follow the live schema and a
            // column the bundle does not carry takes its default.
            let conflict = match mode {
                ImportMode::Replace => "",
                ImportMode::Merge => " ON CONFLICT DO NOTHING",
            };
            for chunk in raw.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().chunks(2000) {
                let array = format!("[{}]", chunk.join(","));
                let sql = format!(
                    "INSERT INTO {table} SELECT * FROM json_populate_recordset(NULL::{table}, $1::json){conflict}"
                );
                let done = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(&array)
                    .execute(&mut *tx)
                    .await
                    .with_context(|| format!("loading {table}"))?;
                rows_loaded += done.rows_affected();
            }

            resync_sequences(&mut tx, table).await?;
        }

        report.push(ImportedModule {
            id: id.clone(),
            name: name.to_string(),
            rows: rows_loaded,
            replaced: mode == ImportMode::Replace,
        });
    }

    tx.commit().await?;
    Ok(report)
}

/// Rows carry their original ids, so every generated column has to be pushed past the
/// highest value loaded. Without this the next insert collides on the primary key.
async fn resync_sequences(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
) -> anyhow::Result<()> {
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 \
           AND (column_default LIKE 'nextval%' OR is_identity = 'YES')",
    )
    .bind(table)
    .fetch_all(&mut **tx)
    .await
    .with_context(|| format!("listing generated columns of {table}"))?;

    for col in columns {
        // Identifiers here come from the catalog for a table we already vetted, never from
        // the bundle. setval to MAX(col), or leave the sequence at 1 on an empty table.
        let sql = format!(
            "SELECT setval(pg_get_serial_sequence('{table}', '{col}'), \
                    GREATEST(COALESCE((SELECT MAX({col}) FROM {table}), 0), 1))"
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut **tx)
            .await
            .with_context(|| format!("resyncing {table}.{col}"))?;
    }
    Ok(())
}

async fn table_exists(pool: &PgPool, table: &str) -> anyhow::Result<bool> {
    let row = sqlx::query("SELECT to_regclass($1) IS NOT NULL AS present")
        .bind(table)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<bool, _>("present"))
}

async fn table_exists_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
) -> anyhow::Result<bool> {
    let row = sqlx::query("SELECT to_regclass($1) IS NOT NULL AS present")
        .bind(table)
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.get::<bool, _>("present"))
}

/// Compare two `x.y.z` strings. Anything unparsable compares as "not newer", so a nonstandard
/// version string can never block a load on its own.
fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<(u32, u32, u32)> {
        let mut it = v.trim().trim_start_matches('v').split('.');
        Some((
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
            it.next().unwrap_or("0").parse().ok()?,
        ))
    }
    match (parts(candidate), parts(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_from_a_newer_app_is_refused() {
        assert!(is_newer("0.1.0", "0.0.11"));
        assert!(is_newer("0.0.12", "0.0.11"));
        assert!(!is_newer("0.0.11", "0.0.11"));
        assert!(!is_newer("0.0.10", "0.0.11"));
        // Unparsable never blocks a load.
        assert!(!is_newer("dev", "0.0.11"));
    }

    #[test]
    fn credential_stores_are_listed_not_guessed() {
        assert!(CREDENTIAL_TABLES.contains(&"feed_secrets"));
        // A name heuristic would have caught these; the explicit list must not.
        assert!(!CREDENTIAL_TABLES.contains(&"agent_messages"));
    }
}
