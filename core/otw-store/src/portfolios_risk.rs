//! Storage for the portfolio's target allocation, its stress scenarios and the betas they
//! are measured with.
//!
//! Three small tables that exist so three analyzers can be independent: `drift` reads
//! targets, `stress` reads scenarios and betas, and neither knows the other exists.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Targets ───────────────────────────────────────────────────────────────────

/// One bucket of a target allocation.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Target {
    /// `asset_class` today. `sector`, `region`, `currency` are the same table later.
    pub dimension: String,
    pub bucket: String,
    pub target_pct: f64,
    /// How far the bucket may drift before it is out of band, in percentage points.
    pub band_pct: f64,
}

pub async fn list_targets(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Target>> {
    Ok(sqlx::query_as::<_, Target>(
        "SELECT dimension, bucket, target_pct, band_pct FROM portfolio_targets \
         WHERE portfolio_id = $1 ORDER BY dimension, bucket",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("listing targets")?)
}

/// Replace one dimension's targets wholesale.
///
/// Wholesale, not row by row: a target allocation is a single statement that has to sum to
/// 100, and patching one bucket at a time would let the set sit invalid between two calls.
/// An empty list clears the dimension, which is how a user turns the drift table off.
pub async fn set_targets(
    pool: &PgPool,
    portfolio_id: Uuid,
    dimension: &str,
    rows: &[Target],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM portfolio_targets WHERE portfolio_id = $1 AND dimension = $2")
        .bind(portfolio_id)
        .bind(dimension)
        .execute(&mut *tx)
        .await
        .context("clearing targets")?;
    for r in rows {
        sqlx::query(
            "INSERT INTO portfolio_targets (portfolio_id, dimension, bucket, target_pct, band_pct) \
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(portfolio_id)
        .bind(dimension)
        .bind(&r.bucket)
        .bind(r.target_pct)
        .bind(r.band_pct)
        .execute(&mut *tx)
        .await
        .context("writing target")?;
    }
    tx.commit().await?;
    Ok(())
}

// ── Scenarios ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Scenario {
    pub id: Uuid,
    pub name: String,
    /// `factor` (a set of shocks) or `historical` (a window to replay).
    pub kind: String,
    pub slug: Option<String>,
    pub builtin: bool,
    pub legs: serde_json::Value,
    pub note: String,
    pub position: f64,
}

const SCEN_COLS: &str = "id, name, kind, slug, builtin, legs, note, position";

pub async fn list_scenarios(pool: &PgPool) -> anyhow::Result<Vec<Scenario>> {
    let sql = format!("SELECT {SCEN_COLS} FROM portfolio_scenarios ORDER BY position, name");
    Ok(sqlx::query_as::<_, Scenario>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing scenarios")?)
}

pub async fn get_scenario(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Scenario>> {
    let sql = format!("SELECT {SCEN_COLS} FROM portfolio_scenarios WHERE id = $1");
    Ok(sqlx::query_as::<_, Scenario>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching scenario")?)
}

pub async fn upsert_scenario(
    pool: &PgPool,
    id: Option<Uuid>,
    name: &str,
    kind: &str,
    legs: &serde_json::Value,
    note: &str,
) -> anyhow::Result<Scenario> {
    let sql = match id {
        // A builtin keeps its slug and its builtin flag through an edit: the user is allowed
        // to change what a shipped scenario means, and the row still has to be findable by
        // the seeder that owns it.
        Some(_) => format!(
            "UPDATE portfolio_scenarios SET name = $2, kind = $3, legs = $4, note = $5, \
             updated_at = now() WHERE id = $1 RETURNING {SCEN_COLS}"
        ),
        None => format!(
            "INSERT INTO portfolio_scenarios (id, name, kind, legs, note, position) \
             VALUES ($1,$2,$3,$4,$5,(SELECT COALESCE(MAX(position),0)+1 FROM portfolio_scenarios)) \
             RETURNING {SCEN_COLS}"
        ),
    };
    Ok(sqlx::query_as::<_, Scenario>(sqlx::AssertSqlSafe(sql))
        .bind(id.unwrap_or_else(Uuid::new_v4))
        .bind(name)
        .bind(kind)
        .bind(legs)
        .bind(note)
        .fetch_one(pool)
        .await
        .context("saving scenario")?)
}

/// Delete a scenario the user made. A builtin is refused: it would come back on the next
/// migration anyway, and a row that reappears after a delete is worse than one that stays.
pub async fn delete_scenario(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM portfolio_scenarios WHERE id = $1 AND NOT builtin")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting scenario")?
        .rows_affected();
    Ok(n > 0)
}

// ── Betas ─────────────────────────────────────────────────────────────────────

/// A sensitivity that was actually measured, with what it was measured on.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Beta {
    pub asset_id: Uuid,
    pub factor: String,
    pub lookback_days: i32,
    pub beta: f64,
    pub r2: f64,
    pub rows: i32,
    pub watermark: Option<OffsetDateTime>,
}

pub async fn list_betas(pool: &PgPool, portfolio_id: Uuid, lookback_days: i32) -> anyhow::Result<Vec<Beta>> {
    Ok(sqlx::query_as::<_, Beta>(
        "SELECT b.asset_id, b.factor, b.lookback_days, b.beta, b.r2, b.rows, b.watermark \
         FROM portfolio_betas b JOIN portfolio_assets a ON a.id = b.asset_id \
         WHERE a.portfolio_id = $1 AND b.lookback_days = $2",
    )
    .bind(portfolio_id)
    .bind(lookback_days)
    .fetch_all(pool)
    .await
    .context("listing betas")?)
}

#[allow(clippy::too_many_arguments)]
pub async fn set_beta(
    pool: &PgPool,
    asset_id: Uuid,
    factor: &str,
    lookback_days: i32,
    beta: f64,
    r2: f64,
    rows: i32,
    watermark: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO portfolio_betas (asset_id, factor, lookback_days, beta, r2, rows, watermark, computed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7, now()) \
         ON CONFLICT (asset_id, factor, lookback_days) DO UPDATE SET \
           beta = EXCLUDED.beta, r2 = EXCLUDED.r2, rows = EXCLUDED.rows, \
           watermark = EXCLUDED.watermark, computed_at = now()",
    )
    .bind(asset_id)
    .bind(factor)
    .bind(lookback_days)
    .bind(beta)
    .bind(r2)
    .bind(rows)
    .bind(watermark)
    .execute(pool)
    .await
    .context("writing beta")?;
    Ok(())
}

/// Drop a fit that can no longer be measured. A beta cache is a cache: a stale row would be
/// used as if it were current, and a wrong sensitivity is worse than a missing one.
pub async fn forget_beta(pool: &PgPool, asset_id: Uuid, factor: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM portfolio_betas WHERE asset_id = $1 AND factor = $2")
        .bind(asset_id)
        .bind(factor)
        .execute(pool)
        .await
        .context("forgetting beta")?;
    Ok(())
}
