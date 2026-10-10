//! Which routines a journal book runs, and how each day of its calendar reads.
//!
//! The routines themselves belong to the Trading routines module (`trader_routines`,
//! ticked item by item in `trader_routine_checks`). The journal adds one thing: a link
//! saying "this book runs that routine from this day to that one" (`end_date` NULL =
//! unlimited). Nothing is ticked here and nothing is duplicated: a day's status is
//! derived from the routine's own recurrence and its own checks.
//!
//! A routine is *due* on a day when the link's window contains it **and** the routine's
//! recurrence says so (a weekday routine is not owed on a Saturday), and *done* when
//! every one of its items is ticked that day.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use time::Date;
use uuid::Uuid;

use crate::trader_tasks;

/// A routine as it sits on one calendar. Dates travel as `YYYY-MM-DD` strings: they are
/// calendar days, and the client compares them to the keys it builds the month grid from.
#[derive(Debug, Serialize)]
pub struct RoutineLink {
    pub id: Uuid,
    pub routine_id: Uuid,
    pub category_id: Uuid,
    pub name: String,
    pub description: String,
    /// The recurrence in words is the client's job; this is what the module stores.
    pub schedule: Option<sqlx::types::JsonValue>,
    pub weekdays: i32,
    pub start_date: String,
    pub end_date: Option<String>,
}

/// One routine's standing on one day.
#[derive(Debug, Serialize)]
pub struct DayRoutine {
    pub link_id: Uuid,
    pub routine_id: Uuid,
    pub name: String,
    pub done: bool,
}

/// A day that owes at least one routine. Days owing nothing are not returned at all.
#[derive(Debug, Serialize)]
pub struct DayStatus {
    pub date: String,
    pub routines: Vec<DayRoutine>,
}

// ── Links ────────────────────────────────────────────────────────────────────

type LinkRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    String,
    Option<sqlx::types::JsonValue>,
    i32,
    Date,
    Option<Date>,
);

fn row_to_link(r: LinkRow) -> RoutineLink {
    RoutineLink {
        id: r.0,
        routine_id: r.1,
        category_id: r.2,
        name: r.3,
        description: r.4,
        schedule: r.5,
        weekdays: r.6,
        start_date: r.7.to_string(),
        end_date: r.8.map(|d| d.to_string()),
    }
}

/// The routines on one calendar (or every calendar when `category` is None).
pub async fn list_links(pool: &PgPool, category: Option<Uuid>) -> anyhow::Result<Vec<RoutineLink>> {
    let rows = sqlx::query_as::<_, LinkRow>(
        "SELECT l.id, l.routine_id, l.category_id, r.name, r.description, r.schedule, \
                r.weekdays, l.start_date, l.end_date \
         FROM journal_routine_links l \
         JOIN trader_routines r ON r.id = l.routine_id \
         WHERE ($1::uuid IS NULL OR l.category_id = $1) \
         ORDER BY r.position, r.name",
    )
    .bind(category)
    .fetch_all(pool)
    .await
    .context("listing routine links")?;
    Ok(rows.into_iter().map(row_to_link).collect())
}

/// Attach routines to a calendar over one period. Re-attaching an already-linked
/// routine moves its period rather than failing: the user asked for that period.
pub async fn attach(
    pool: &PgPool,
    category_id: Uuid,
    routine_ids: &[Uuid],
    start: Date,
    end: Option<Date>,
) -> anyhow::Result<Vec<RoutineLink>> {
    let mut tx = pool.begin().await?;
    for rid in routine_ids {
        sqlx::query(
            "INSERT INTO journal_routine_links \
                 (id, routine_id, category_id, start_date, end_date) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (routine_id, category_id) \
             DO UPDATE SET start_date = EXCLUDED.start_date, end_date = EXCLUDED.end_date",
        )
        .bind(Uuid::new_v4())
        .bind(rid)
        .bind(category_id)
        .bind(start)
        .bind(end)
        .execute(&mut *tx)
        .await
        .context("attaching routine")?;
    }
    tx.commit().await?;
    list_links(pool, Some(category_id)).await
}

/// Move a link's period. `end` is a double option: absent keeps the stored end, an
/// explicit `None` clears it and the routine runs unlimited.
pub async fn update_link(
    pool: &PgPool,
    id: Uuid,
    start: Option<Date>,
    end: Option<Option<Date>>,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_routine_links SET \
            start_date = COALESCE($2, start_date), \
            end_date = CASE WHEN $3 THEN $4 ELSE end_date END \
         WHERE id = $1",
    )
    .bind(id)
    .bind(start)
    .bind(end.is_some())
    .bind(end.flatten())
    .execute(pool)
    .await
    .context("updating routine link")?;
    Ok(res.rows_affected() > 0)
}

/// Take a routine off a calendar. The routine itself is untouched.
pub async fn detach(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_routine_links WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Daily status ─────────────────────────────────────────────────────────────

/// The days of `[from, to]` that owe at least one routine, each with the routines due
/// and whether they were performed.
///
/// "Performed" is read from the Trading routines module: every item of the routine
/// ticked that day. A routine with no item can never be done, which is what makes an
/// empty checklist visible rather than silently green.
pub async fn status(
    pool: &PgPool,
    category: Uuid,
    from: Date,
    to: Date,
) -> anyhow::Result<Vec<DayStatus>> {
    let links = list_links(pool, Some(category)).await?;
    if links.is_empty() || from > to {
        return Ok(Vec::new());
    }

    // The routine rows themselves, for the recurrence. The table is small and the
    // module already reads it whole.
    let by_id: HashMap<Uuid, trader_tasks::Routine> = trader_tasks::list_routines(pool)
        .await?
        .into_iter()
        .map(|r| (r.id, r))
        .collect();

    let ids: Vec<Uuid> = links.iter().map(|l| l.routine_id).collect();
    let done_rows = sqlx::query_as::<_, (Uuid, Date)>(
        "SELECT i.routine_id, c.check_date \
         FROM trader_routine_items i \
         JOIN trader_routine_checks c ON c.item_id = i.id \
         WHERE i.routine_id = ANY($1) AND c.check_date BETWEEN $2 AND $3 \
         GROUP BY i.routine_id, c.check_date \
         HAVING COUNT(*) = (SELECT COUNT(*) FROM trader_routine_items x \
                            WHERE x.routine_id = i.routine_id)",
    )
    .bind(&ids)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .context("reading routine checks")?;
    let done: HashSet<(Uuid, Date)> = done_rows.into_iter().collect();

    // Parsed once per link, not once per day.
    let windows: Vec<(&RoutineLink, Date, Option<Date>)> = links
        .iter()
        .filter_map(|l| {
            let start = Date::parse(&l.start_date, &time::format_description::well_known::Iso8601::DATE).ok()?;
            let end = match &l.end_date {
                Some(e) => Some(
                    Date::parse(e, &time::format_description::well_known::Iso8601::DATE).ok()?,
                ),
                None => None,
            };
            Some((l, start, end))
        })
        .collect();

    let mut out = Vec::new();
    let mut day = from;
    while day <= to {
        let mut routines = Vec::new();
        for (link, start, end) in &windows {
            if day < *start || end.is_some_and(|e| day > e) {
                continue;
            }
            let Some(routine) = by_id.get(&link.routine_id) else {
                continue;
            };
            // The routine's own recurrence decides: a weekday routine owes nothing on
            // a Saturday, even inside the link's window.
            if !trader_tasks::occurs(routine, day) {
                continue;
            }
            routines.push(DayRoutine {
                link_id: link.id,
                routine_id: link.routine_id,
                name: link.name.clone(),
                done: done.contains(&(link.routine_id, day)),
            });
        }
        if !routines.is_empty() {
            out.push(DayStatus {
                date: day.to_string(),
                routines,
            });
        }
        day = day.next_day().unwrap_or(day);
        if day == to && out.len() > 20_000 {
            break; // a runaway window is a bug, not a request to allocate forever
        }
    }
    Ok(out)
}
