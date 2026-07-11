//! Storage for the Calendar module — personal events.
//!
//! Only the user's own events are persisted; the Economics and Earnings tabs are
//! embedded investing.com widgets and store nothing. Single-user: no owner scoping.
//!
//! Timestamps cross the wire as RFC3339 strings (the format FullCalendar emits and
//! consumes). All-day events carry midnight times with `all_day = true`.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct CalendarEvent {
    pub id: Uuid,
    pub title: String,
    #[serde(with = "ts")]
    pub start_at: OffsetDateTime,
    #[serde(with = "ts_opt")]
    pub end_at: Option<OffsetDateTime>,
    pub all_day: bool,
    pub category: String,
    pub color: String,
    pub location: String,
    pub notes: String,
}

mod ts {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

mod ts_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct CalendarEventInput {
    #[serde(default)]
    pub title: String,
    /// RFC3339 timestamp.
    pub start_at: String,
    /// RFC3339 timestamp, or null/empty for none.
    pub end_at: Option<String>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
}

impl CalendarEventInput {
    /// Parsed start, or an error if the RFC3339 string is missing/malformed.
    pub fn start(&self) -> anyhow::Result<OffsetDateTime> {
        OffsetDateTime::parse(self.start_at.trim(), &Rfc3339).context("invalid start_at")
    }
    pub fn end(&self) -> Option<OffsetDateTime> {
        self.end_at
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
    }
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: Uuid,
    title: String,
    start_at: OffsetDateTime,
    end_at: Option<OffsetDateTime>,
    all_day: bool,
    category: String,
    color: String,
    location: String,
    notes: String,
}

fn row_to_event(r: EventRow) -> CalendarEvent {
    CalendarEvent {
        id: r.id,
        title: r.title,
        start_at: r.start_at,
        end_at: r.end_at,
        all_day: r.all_day,
        category: r.category,
        color: r.color,
        location: r.location,
        notes: r.notes,
    }
}

const COLUMNS: &str =
    "id, title, start_at, end_at, all_day, category, color, location, notes";

/// List events overlapping the `[from, to)` window (RFC3339), or all if either is None.
pub async fn list_events(
    pool: &PgPool,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let rows = match (from, to) {
        (Some(f), Some(t)) => {
            let sql = format!(
                "SELECT {COLUMNS} FROM calendar_events \
                 WHERE start_at < $2 AND COALESCE(end_at, start_at) >= $1 \
                 ORDER BY start_at"
            );
            sqlx::query_as::<_, EventRow>(sqlx::AssertSqlSafe(sql))
                .bind(f)
                .bind(t)
                .fetch_all(pool)
                .await
        }
        _ => {
            let sql = format!("SELECT {COLUMNS} FROM calendar_events ORDER BY start_at");
            sqlx::query_as::<_, EventRow>(sqlx::AssertSqlSafe(sql))
                .fetch_all(pool)
                .await
        }
    }
    .context("listing calendar events")?;
    Ok(rows.into_iter().map(row_to_event).collect())
}

pub async fn get_event(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<CalendarEvent>> {
    let sql = format!("SELECT {COLUMNS} FROM calendar_events WHERE id = $1");
    let row = sqlx::query_as::<_, EventRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching calendar event")?;
    Ok(row.map(row_to_event))
}

pub async fn add_event(
    pool: &PgPool,
    input: &CalendarEventInput,
) -> anyhow::Result<CalendarEvent> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO calendar_events \
         (id, title, start_at, end_at, all_day, category, color, location, notes) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(&input.title)
    .bind(input.start()?)
    .bind(input.end())
    .bind(input.all_day)
    .bind(&input.category)
    .bind(&input.color)
    .bind(&input.location)
    .bind(&input.notes)
    .execute(pool)
    .await
    .context("inserting calendar event")?;
    get_event(pool, id)
        .await?
        .context("event vanished after insert")
}

pub async fn update_event(
    pool: &PgPool,
    id: Uuid,
    input: &CalendarEventInput,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE calendar_events SET \
         title = $2, start_at = $3, end_at = $4, all_day = $5, category = $6, \
         color = $7, location = $8, notes = $9, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&input.title)
    .bind(input.start()?)
    .bind(input.end())
    .bind(input.all_day)
    .bind(&input.category)
    .bind(&input.color)
    .bind(&input.location)
    .bind(&input.notes)
    .execute(pool)
    .await
    .context("updating calendar event")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_event(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM calendar_events WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
