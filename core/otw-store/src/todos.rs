//! Storage for the ToDo module.
//!
//! A flat task list: name, optional due date, free-form details, and a done flag.
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Time};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Todo {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "date_opt")]
    pub due_date: Option<Date>,
    /// Optional clock time for the due date (display only); `HH:MM`.
    #[serde(with = "time_opt")]
    pub due_time: Option<Time>,
    pub details: String,
    pub done: bool,
    /// Free-text grouping label; empty string means uncategorized.
    pub category: String,
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

mod time_opt {
    use serde::Serializer;
    use time::{format_description::FormatItem, macros::format_description, Time};
    const HHMM: &[FormatItem<'_>] = format_description!("[hour]:[minute]");
    pub fn serialize<S: Serializer>(t: &Option<Time>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format(HHMM).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct TodoInput {
    #[serde(default)]
    pub name: String,
    /// `YYYY-MM-DD`, or null/empty for no due date.
    pub due_date: Option<String>,
    /// `HH:MM`, or null/empty for no time.
    pub due_time: Option<String>,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub category: String,
}

impl TodoInput {
    fn due(&self) -> Option<Date> {
        self.due_date
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
    }
    fn due_time_val(&self) -> Option<Time> {
        self.due_time
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(parse_hhmm)
    }
}

/// Parse an `HH:MM` (or `HH:MM:SS`) clock time.
fn parse_hhmm(s: &str) -> Option<Time> {
    let mut it = s.split(':');
    let h: u8 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    Time::from_hms(h, m, 0).ok()
}

#[derive(sqlx::FromRow)]
struct TodoRow {
    id: Uuid,
    name: String,
    due_date: Option<Date>,
    due_time: Option<Time>,
    details: String,
    done: bool,
    category: String,
}

fn row_to_todo(r: TodoRow) -> Todo {
    Todo {
        id: r.id,
        name: r.name,
        due_date: r.due_date,
        due_time: r.due_time,
        details: r.details,
        done: r.done,
        category: r.category,
    }
}

const COLUMNS: &str = "id, name, due_date, due_time, details, done, category";

pub async fn list_todos(pool: &PgPool) -> anyhow::Result<Vec<Todo>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM todos \
         ORDER BY done, due_date NULLS LAST, created_at"
    );
    let rows = sqlx::query_as::<_, TodoRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing todos")?;
    Ok(rows.into_iter().map(row_to_todo).collect())
}

pub async fn get_todo(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Todo>> {
    let sql = format!("SELECT {COLUMNS} FROM todos WHERE id = $1");
    let row = sqlx::query_as::<_, TodoRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching todo")?;
    Ok(row.map(row_to_todo))
}

pub async fn add_todo(pool: &PgPool, input: &TodoInput) -> anyhow::Result<Todo> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO todos (id, name, due_date, due_time, details, done, category) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.due())
    .bind(input.due_time_val())
    .bind(&input.details)
    .bind(input.done)
    .bind(&input.category)
    .execute(pool)
    .await
    .context("inserting todo")?;
    get_todo(pool, id).await?.context("todo vanished after insert")
}

pub async fn update_todo(pool: &PgPool, id: Uuid, input: &TodoInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE todos SET name = $2, due_date = $3, due_time = $4, details = $5, done = $6, \
         category = $7, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.due())
    .bind(input.due_time_val())
    .bind(&input.details)
    .bind(input.done)
    .bind(&input.category)
    .execute(pool)
    .await
    .context("updating todo")?;
    Ok(res.rows_affected() > 0)
}

/// Toggle just the done flag (used by the checkbox in the list).
pub async fn set_done(pool: &PgPool, id: Uuid, done: bool) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE todos SET done = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(done)
        .execute(pool)
        .await
        .context("toggling todo")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_todo(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM todos WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
