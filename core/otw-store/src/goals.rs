//! Storage for the Goals module.
//!
//! A goal has a name, optional deadline, free-form details, and a JSONB array of KPIs.
//! Progress is derived as reached-points / total-points. KPIs are stored as JSONB (like
//! the wealth `fields` column) and validated in the API layer. Single-user.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Goal {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "date_opt")]
    pub deadline: Option<Date>,
    pub details: String,
    /// Array of KPI objects: { name, target, current, reached, points }.
    pub kpis: JsonValue,
    /// Free-text grouping label; empty string means uncategorized.
    pub category: String,
    /// Manual sort order (drag-to-reorder). Lower comes first.
    pub position: f64,
}

mod date_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GoalInput {
    #[serde(default)]
    pub name: String,
    /// `YYYY-MM-DD`, or null/empty for no deadline.
    pub deadline: Option<String>,
    #[serde(default)]
    pub details: String,
    #[serde(default = "empty_array")]
    pub kpis: JsonValue,
    #[serde(default)]
    pub category: String,
}

fn empty_array() -> JsonValue {
    JsonValue::Array(Vec::new())
}

impl GoalInput {
    fn deadline_date(&self) -> Option<Date> {
        self.deadline
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
    }
}

#[derive(sqlx::FromRow)]
struct GoalRow {
    id: Uuid,
    name: String,
    deadline: Option<Date>,
    details: String,
    kpis: JsonValue,
    category: String,
    position: f64,
}

fn row_to_goal(r: GoalRow) -> Goal {
    Goal {
        id: r.id,
        name: r.name,
        deadline: r.deadline,
        details: r.details,
        kpis: r.kpis,
        category: r.category,
        position: r.position,
    }
}

const COLUMNS: &str = "id, name, deadline, details, kpis, category, position";

pub async fn list_goals(pool: &PgPool) -> anyhow::Result<Vec<Goal>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM goals ORDER BY position, created_at"
    );
    let rows = sqlx::query_as::<_, GoalRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing goals")?;
    Ok(rows.into_iter().map(row_to_goal).collect())
}

pub async fn get_goal(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Goal>> {
    let sql = format!("SELECT {COLUMNS} FROM goals WHERE id = $1");
    let row = sqlx::query_as::<_, GoalRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching goal")?;
    Ok(row.map(row_to_goal))
}

pub async fn add_goal(pool: &PgPool, input: &GoalInput) -> anyhow::Result<Goal> {
    let id = Uuid::new_v4();
    // Append new goals to the end of the manual order.
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM goals")
        .fetch_one(pool)
        .await
        .context("querying max goal position")?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO goals (id, name, deadline, details, kpis, category, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.deadline_date())
    .bind(&input.details)
    .bind(&input.kpis)
    .bind(&input.category)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting goal")?;
    get_goal(pool, id).await?.context("goal vanished after insert")
}

pub async fn set_position(pool: &PgPool, id: Uuid, position: f64) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE goals SET position = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(position)
        .execute(pool)
        .await
        .context("updating goal position")?;
    Ok(res.rows_affected() > 0)
}

pub async fn update_goal(pool: &PgPool, id: Uuid, input: &GoalInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE goals SET name = $2, deadline = $3, details = $4, kpis = $5, category = $6, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.deadline_date())
    .bind(&input.details)
    .bind(&input.kpis)
    .bind(&input.category)
    .execute(pool)
    .await
    .context("updating goal")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_goal(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM goals WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
