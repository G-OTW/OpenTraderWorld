//! Storage for the Trader Tasks module.
//!
//! Routines are recurring checklists tied to a part of the trading day (pre-market, in
//! session, post-market, anytime) and due on a weekday bitmask (Mon=1 … Sun=64). Ticks are
//! per (item, date) rows, so every day starts fresh and history is queryable. Quick tasks
//! are one-off todos with an optional due date and a priority. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Routine {
    pub id: Uuid,
    pub name: String,
    pub session: String,
    pub weekdays: i32,
    pub position: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RoutineItem {
    pub id: Uuid,
    pub routine_id: Uuid,
    pub label: String,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub note: String,
    pub priority: String,
    #[serde(with = "date_opt")]
    pub due_date: Option<Date>,
    pub done: bool,
}

/// A routine with its items and their tick state for one date.
#[derive(Debug, Clone, Serialize)]
pub struct RoutineView {
    #[serde(flatten)]
    pub routine: Routine,
    pub items: Vec<ItemView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemView {
    #[serde(flatten)]
    pub item: RoutineItem,
    pub checked: bool,
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

// ── Starter routines ─────────────────────────────────────────────────────────

/// (name, session, weekdays, items) for the seeded starter set. Weekday mask 31 = Mon–Fri.
///
/// The trader-wisdom routines are paraphrased from public principles and credited by name
/// only — ideas aren't copyrightable and we quote nothing. They install once as normal,
/// fully-editable routines the user can rename, tweak, or delete like their own.
fn default_routines() -> Vec<(&'static str, &'static str, i32, Vec<&'static str>)> {
    vec![
        (
            "Pre-market prep",
            "pre",
            31,
            vec![
                "Review overnight news & futures",
                "Check the economic calendar for releases",
                "Mark key levels on your watchlist",
                "Define max loss for the day",
                "Write your plan: setups you will (and will not) take",
            ],
        ),
        (
            "In-session discipline",
            "live",
            31,
            vec![
                "Only take planned setups",
                "Respect position size rules",
                "Log every trade as you take it",
                "Step away after 2 consecutive losses",
            ],
        ),
        (
            "Post-market review",
            "post",
            31,
            vec![
                "Journal every trade (screenshots + reasoning)",
                "Grade your execution, not the outcome",
                "Note one thing to improve tomorrow",
                "Close the platform — no revenge trading",
            ],
        ),
        // ── Trader-wisdom routines (name-attributed, paraphrased) ────────────────
        (
            "Pre-market — defense first (Paul Tudor Jones)",
            "pre",
            31,
            vec![
                "Check trend vs the 200-day: only lean with the trend",
                "For every idea, ask: how can I lose on this?",
                "Predefine the stop before the entry",
                "Playing great defense — protect capital before chasing offense",
                "If the month is deep in the red, cut size hard",
            ],
        ),
        (
            "Pre-market — champion setup scan (Mark Minervini)",
            "pre",
            31,
            vec![
                "Scan for textbook setups — wait in cash until one appears",
                "Confirm the general market trend is supportive",
                "Set the hard stop (never let a loss run past your line)",
                "Size so risk-per-trade stays small and consistent",
                "No setup, no trade — patience is a position",
            ],
        ),
        (
            "Pre-market — rituals (Linda Raschke)",
            "pre",
            31,
            vec![
                "Same start time, same prep sequence every day",
                "Review yesterday's tape and open positions",
                "Note market internals and the day's likely regime",
                "Mentally rehearse: if X, I do Y",
                "Set a realistic goal for the session and stop",
            ],
        ),
        (
            "In-session — mechanical rules (Turtles / Richard Dennis)",
            "live",
            31,
            vec![
                "Follow the system's entry signal — no discretion",
                "Take every valid signal (missing winners is the real risk)",
                "Size by volatility, not by conviction",
                "Exit on the rule, not on a feeling",
                "Don't add to a loser outside the plan",
            ],
        ),
        (
            "Post-market — grade the process (Mark Minervini)",
            "post",
            31,
            vec![
                "Review each trade against its plan and stop",
                "Grade execution, not PnL",
                "Tag any rule breaks and the emotion behind them",
                "Log the one adjustment for tomorrow",
                "Update your watchlist for the next session",
            ],
        ),
    ]
}

/// Settings flag marking that the starter routines were installed once. One-shot so a user
/// who deletes them isn't refilled on the next board load.
const SEEDED_KEY: &str = "trader_routines_seeded";

/// Install the starter routines on first use only (never after the user has touched the set).
pub async fn seed_if_first_run(pool: &PgPool) -> anyhow::Result<()> {
    if crate::settings::get(pool, SEEDED_KEY).await?.is_some() {
        return Ok(());
    }
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM trader_routines")
        .fetch_one(pool)
        .await
        .context("counting routines")?;
    if count == 0 {
        for (name, session, weekdays, items) in default_routines() {
            let labels: Vec<String> = items.into_iter().map(str::to_owned).collect();
            create_routine(pool, name, session, weekdays, &labels).await?;
        }
    }
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

// ── Routines ─────────────────────────────────────────────────────────────────

pub async fn list_routines(pool: &PgPool) -> anyhow::Result<Vec<Routine>> {
    Ok(sqlx::query_as::<_, Routine>(
        "SELECT id, name, session, weekdays, position, active FROM trader_routines \
         ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing routines")?)
}

pub async fn get_routine(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Routine>> {
    Ok(sqlx::query_as::<_, Routine>(
        "SELECT id, name, session, weekdays, position, active FROM trader_routines WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching routine")?)
}

pub async fn create_routine(
    pool: &PgPool,
    name: &str,
    session: &str,
    weekdays: i32,
    items: &[String],
) -> anyhow::Result<Routine> {
    let routine = sqlx::query_as::<_, Routine>(
        "INSERT INTO trader_routines (id, name, session, weekdays, position) \
         VALUES ($1,$2,$3,$4, (SELECT COALESCE(MAX(position),0)+1 FROM trader_routines)) \
         RETURNING id, name, session, weekdays, position, active",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(session)
    .bind(weekdays)
    .fetch_one(pool)
    .await
    .context("creating routine")?;
    for (i, label) in items.iter().enumerate() {
        add_item(pool, routine.id, label, i as f64).await?;
    }
    Ok(routine)
}

pub async fn update_routine(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    session: Option<&str>,
    weekdays: Option<i32>,
    active: Option<bool>,
) -> anyhow::Result<Option<Routine>> {
    Ok(sqlx::query_as::<_, Routine>(
        "UPDATE trader_routines SET \
         name = COALESCE($2, name), \
         session = COALESCE($3, session), \
         weekdays = COALESCE($4, weekdays), \
         active = COALESCE($5, active) \
         WHERE id = $1 RETURNING id, name, session, weekdays, position, active",
    )
    .bind(id)
    .bind(name)
    .bind(session)
    .bind(weekdays)
    .bind(active)
    .fetch_optional(pool)
    .await
    .context("updating routine")?)
}

pub async fn delete_routine(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM trader_routines WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting routine")?;
    Ok(())
}

// ── Routine items ────────────────────────────────────────────────────────────

pub async fn add_item(
    pool: &PgPool,
    routine_id: Uuid,
    label: &str,
    position: f64,
) -> anyhow::Result<RoutineItem> {
    Ok(sqlx::query_as::<_, RoutineItem>(
        "INSERT INTO trader_routine_items (id, routine_id, label, position) \
         VALUES ($1,$2,$3,$4) RETURNING id, routine_id, label, position",
    )
    .bind(Uuid::new_v4())
    .bind(routine_id)
    .bind(label)
    .bind(position)
    .fetch_one(pool)
    .await
    .context("adding routine item")?)
}

/// Replace a routine's items with `items` (id-keeping): entries carrying an id update that
/// item's label/position; entries without one are inserted; existing items not referenced
/// are deleted (with their tick history — they no longer exist).
pub async fn set_items(
    pool: &PgPool,
    routine_id: Uuid,
    items: &[(Option<Uuid>, String)],
) -> anyhow::Result<()> {
    let keep: Vec<Uuid> = items.iter().filter_map(|(id, _)| *id).collect();
    sqlx::query("DELETE FROM trader_routine_items WHERE routine_id = $1 AND NOT (id = ANY($2))")
        .bind(routine_id)
        .bind(&keep)
        .execute(pool)
        .await
        .context("pruning routine items")?;
    for (i, (id, label)) in items.iter().enumerate() {
        match id {
            Some(id) => {
                sqlx::query(
                    "UPDATE trader_routine_items SET label = $2, position = $3 \
                     WHERE id = $1 AND routine_id = $4",
                )
                .bind(id)
                .bind(label)
                .bind(i as f64)
                .bind(routine_id)
                .execute(pool)
                .await
                .context("updating routine item")?;
            }
            None => {
                add_item(pool, routine_id, label, i as f64).await?;
            }
        }
    }
    Ok(())
}

/// Tick or untick one item for one date.
pub async fn set_check(pool: &PgPool, item_id: Uuid, date: Date, checked: bool) -> anyhow::Result<()> {
    if checked {
        sqlx::query(
            "INSERT INTO trader_routine_checks (item_id, check_date) VALUES ($1,$2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(item_id)
        .bind(date)
        .execute(pool)
        .await
        .context("ticking item")?;
    } else {
        sqlx::query("DELETE FROM trader_routine_checks WHERE item_id = $1 AND check_date = $2")
            .bind(item_id)
            .bind(date)
            .execute(pool)
            .await
            .context("unticking item")?;
    }
    Ok(())
}

// ── Board (one day's view) ───────────────────────────────────────────────────

/// Bit for a date's weekday in the routine mask (Mon=1 … Sun=64).
pub fn weekday_bit(date: Date) -> i32 {
    1 << date.weekday().number_days_from_monday()
}

/// Active routines due on `date` (weekday mask), with items and their tick state.
pub async fn board_routines(pool: &PgPool, date: Date) -> anyhow::Result<Vec<RoutineView>> {
    let bit = weekday_bit(date);
    let routines = sqlx::query_as::<_, Routine>(
        "SELECT id, name, session, weekdays, position, active FROM trader_routines \
         WHERE active AND (weekdays & $1) <> 0 ORDER BY position, created_at",
    )
    .bind(bit)
    .fetch_all(pool)
    .await
    .context("listing due routines")?;

    let mut out = Vec::with_capacity(routines.len());
    for r in routines {
        let items = sqlx::query_as::<_, (Uuid, Uuid, String, f64, bool)>(
            "SELECT i.id, i.routine_id, i.label, i.position, \
                    EXISTS(SELECT 1 FROM trader_routine_checks c \
                           WHERE c.item_id = i.id AND c.check_date = $2) \
             FROM trader_routine_items i WHERE i.routine_id = $1 ORDER BY i.position",
        )
        .bind(r.id)
        .bind(date)
        .fetch_all(pool)
        .await
        .context("listing routine items")?
        .into_iter()
        .map(|(id, routine_id, label, position, checked)| ItemView {
            item: RoutineItem { id, routine_id, label, position },
            checked,
        })
        .collect();
        out.push(RoutineView { routine: r, items });
    }
    Ok(out)
}

/// Items of a routine (for the manage/edit form).
pub async fn list_items(pool: &PgPool, routine_id: Uuid) -> anyhow::Result<Vec<RoutineItem>> {
    Ok(sqlx::query_as::<_, RoutineItem>(
        "SELECT id, routine_id, label, position FROM trader_routine_items \
         WHERE routine_id = $1 ORDER BY position",
    )
    .bind(routine_id)
    .fetch_all(pool)
    .await
    .context("listing items")?)
}

/// Dates with at least one tick in the trailing `days` window ending at `until` —
/// feeds the streak/consistency strip.
pub async fn tick_dates(pool: &PgPool, until: Date, days: i32) -> anyhow::Result<Vec<Date>> {
    Ok(sqlx::query_scalar::<_, Date>(
        "SELECT DISTINCT check_date FROM trader_routine_checks \
         WHERE check_date <= $1 AND check_date > $1 - ($2 || ' days')::interval \
         ORDER BY check_date",
    )
    .bind(until)
    .bind(days.to_string())
    .fetch_all(pool)
    .await
    .context("listing tick dates")?)
}

// ── Quick tasks ──────────────────────────────────────────────────────────────

const TASK_COLS: &str = "id, title, note, priority, due_date, done";

/// Open tasks plus those completed on `date` (so today's finished work stays visible).
pub async fn board_tasks(pool: &PgPool, date: Date) -> anyhow::Result<Vec<Task>> {
    let sql = format!(
        "SELECT {TASK_COLS} FROM trader_tasks \
         WHERE NOT done OR done_at::date = $1 \
         ORDER BY done, CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, \
                  due_date NULLS LAST, created_at"
    );
    Ok(sqlx::query_as::<_, Task>(sqlx::AssertSqlSafe(sql))
        .bind(date)
        .fetch_all(pool)
        .await
        .context("listing tasks")?)
}

pub async fn add_task(
    pool: &PgPool,
    title: &str,
    note: &str,
    priority: &str,
    due_date: Option<Date>,
) -> anyhow::Result<Task> {
    let sql = format!(
        "INSERT INTO trader_tasks (id, title, note, priority, due_date) \
         VALUES ($1,$2,$3,$4,$5) RETURNING {TASK_COLS}"
    );
    Ok(sqlx::query_as::<_, Task>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(title)
        .bind(note)
        .bind(priority)
        .bind(due_date)
        .fetch_one(pool)
        .await
        .context("adding task")?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_task(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    note: Option<&str>,
    priority: Option<&str>,
    due_date: Option<Option<Date>>,
    done: Option<bool>,
) -> anyhow::Result<Option<Task>> {
    let done_at: Option<Option<OffsetDateTime>> =
        done.map(|d| if d { Some(OffsetDateTime::now_utc()) } else { None });
    let sql = format!(
        "UPDATE trader_tasks SET \
         title = COALESCE($2, title), \
         note = COALESCE($3, note), \
         priority = COALESCE($4, priority), \
         due_date = CASE WHEN $5 THEN $6 ELSE due_date END, \
         done = COALESCE($7, done), \
         done_at = CASE WHEN $8 THEN $9 ELSE done_at END \
         WHERE id = $1 RETURNING {TASK_COLS}"
    );
    Ok(sqlx::query_as::<_, Task>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(title)
        .bind(note)
        .bind(priority)
        .bind(due_date.is_some())
        .bind(due_date.flatten())
        .bind(done)
        .bind(done_at.is_some())
        .bind(done_at.flatten())
        .fetch_optional(pool)
        .await
        .context("updating task")?)
}

pub async fn delete_task(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM trader_tasks WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting task")?;
    Ok(())
}
