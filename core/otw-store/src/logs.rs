//! Application logs persisted to `app_logs`, plus query/retention helpers.
//!
//! A custom tracing layer (in otw-core) feeds rows here via `insert`; the Settings "Logs"
//! section reads them via `list`. Retention is bounded by `trim` on a cadence. The
//! minimum captured level is a runtime-adjustable global (`min_level`), so the Settings
//! log-level changer takes effect without a restart.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;
use std::sync::atomic::{AtomicU8, Ordering};

/// Keep at most this many rows; older ones are trimmed.
pub const RETENTION_ROWS: i64 = 50_000;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LogRow {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub level: String,
    pub target: String,
    pub message: String,
}

// ── Runtime minimum level ────────────────────────────────────────────────────
// Ordered most→least severe: error=0 .. trace=4. A log is captured when its
// severity index <= the configured threshold.

static MIN_LEVEL: AtomicU8 = AtomicU8::new(2); // default: info

/// Map a level name to its severity index, or `None` if unknown.
pub fn level_index(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "error" => Some(0),
        "warn" | "warning" => Some(1),
        "info" => Some(2),
        "debug" => Some(3),
        "trace" => Some(4),
        _ => None,
    }
}

pub fn level_name(index: u8) -> &'static str {
    match index {
        0 => "error",
        1 => "warn",
        2 => "info",
        3 => "debug",
        _ => "trace",
    }
}

/// Set the minimum captured level by name (e.g. "warn"). Returns false if unknown.
pub fn set_min_level(name: &str) -> bool {
    match level_index(name) {
        Some(i) => {
            MIN_LEVEL.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn min_level() -> u8 {
    MIN_LEVEL.load(Ordering::Relaxed)
}

pub fn min_level_name() -> &'static str {
    level_name(min_level())
}

/// True if an event at `level` should be captured given the current threshold.
pub fn should_capture(level: &str) -> bool {
    level_index(level).is_some_and(|i| i <= min_level())
}

// ── Persistence ──────────────────────────────────────────────────────────────

pub async fn insert(
    pool: &PgPool,
    level: &str,
    target: &str,
    message: &str,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO app_logs (level, target, message) VALUES ($1, $2, $3)")
        .bind(level)
        .bind(target)
        .bind(message)
        .execute(pool)
        .await
        .context("inserting log row")?;
    Ok(())
}

/// List recent logs, newest first, optionally filtered by minimum severity and a search
/// substring over message/target. `limit` is clamped by the caller.
pub async fn list(
    pool: &PgPool,
    min_level: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<LogRow>> {
    // Filter by severity in Rust-friendly SQL: compare against the set of allowed names.
    let allowed: Vec<&str> = match min_level.and_then(level_index) {
        Some(threshold) => (0..=threshold).map(level_name).collect(),
        None => vec!["error", "warn", "info", "debug", "trace"],
    };
    let like = search
        .map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")))
        .unwrap_or_else(|| "%".to_string());

    let rows = sqlx::query_as::<_, LogRow>(
        "SELECT id, at, level, target, message FROM app_logs \
         WHERE level = ANY($1) AND (message ILIKE $2 OR target ILIKE $2) \
         ORDER BY at DESC, id DESC LIMIT $3",
    )
    .bind(&allowed)
    .bind(&like)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing logs")?;
    Ok(rows)
}

/// Delete all logs. Returns rows removed.
pub async fn clear(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM app_logs").execute(pool).await?;
    Ok(res.rows_affected())
}

/// Trim to the newest `RETENTION_ROWS` rows.
pub async fn trim(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM app_logs WHERE id NOT IN \
         (SELECT id FROM app_logs ORDER BY id DESC LIMIT $1)",
    )
    .bind(RETENTION_ROWS)
    .execute(pool)
    .await
    .context("trimming logs")?;
    Ok(())
}
