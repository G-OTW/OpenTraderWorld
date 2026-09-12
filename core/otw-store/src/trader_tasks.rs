//! Storage for the Trader Tasks module.
//!
//! A routine is a **template**: a named, categorised checklist with a recurrence rule and
//! an optional active window. Ticks are per (item, date) rows, so every day starts fresh
//! and history is queryable. Quick tasks are one-off todos with an optional due date and a
//! priority. Single-user: no owner scoping.
//!
//! Recurrence lives in `schedule` (JSONB) rather than a column per rule, so "every Thursday",
//! "1st of the month" and "weekdays only" are the same shape to store and to query. It is
//! evaluated in Rust ([`Schedule::occurs_on`]) — the rules are cheap and a SQL translation
//! would have to be rewritten for every new kind. The legacy `weekdays` mask is still
//! written alongside so an older client (and the MCP catalog) keeps reading a routine.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Month, OffsetDateTime, Weekday};
use uuid::Uuid;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Routine {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub notes: String,
    pub session: String,
    pub category_id: Option<Uuid>,
    pub weekdays: i32,
    pub schedule: Option<sqlx::types::Json<Schedule>>,
    #[serde(with = "date_opt")]
    pub start_date: Option<Date>,
    #[serde(with = "date_opt")]
    pub end_date: Option<Date>,
    pub position: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RoutineItem {
    pub id: Uuid,
    pub routine_id: Uuid,
    pub label: String,
    pub note: String,
    pub url: String,
    pub link_label: String,
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

// ── Recurrence ───────────────────────────────────────────────────────────────

/// How often a template comes due. Serialised as `{"kind":"weekly","weekdays":31,…}`.
///
/// `interval` counts periods from `anchor` (the routine's `start_date`, or the epoch
/// Monday when it has none) so "every other week" is stable across the year rather than
/// drifting with whatever date the board happens to ask about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// Every `interval` days.
    Daily {
        #[serde(default = "one")]
        interval: i64,
    },
    /// Weekday mask (Mon=1 … Sun=64), every `interval` weeks.
    Weekly {
        weekdays: i32,
        #[serde(default = "one")]
        interval: i64,
    },
    /// Given days of the month; -1 means the last day.
    Monthly { days: Vec<i32> },
    /// The `nth` `weekday` of each month (Mon=0 … Sun=6); `nth` = -1 is the last one.
    NthWeekday { weekday: i32, nth: i32 },
    /// A single date.
    Once {
        // `schemars(with)` is required because the derive would otherwise read the serde
        // `with` module as the field's type.
        #[serde(with = "date_str")]
        #[schemars(with = "String")]
        date: Date,
    },
}

fn one() -> i64 {
    1
}

mod date_str {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let s = String::deserialize(d)?;
        Date::parse(&s, &Iso8601::DATE).map_err(serde::de::Error::custom)
    }
}

/// The Monday all week/day interval counting is measured from when a routine has no
/// `start_date` — 1970-01-05, the first Monday of the epoch.
fn epoch_monday() -> Date {
    Date::from_calendar_date(1970, Month::January, 5).expect("valid constant date")
}

impl Schedule {
    /// The weekday mask this rule implies, for the legacy `weekdays` column and for
    /// clients that still read it. Rules that aren't weekly report every day.
    pub fn weekday_mask(&self) -> i32 {
        match self {
            Schedule::Weekly { weekdays, .. } => *weekdays,
            Schedule::Once { date } => weekday_bit(*date),
            Schedule::NthWeekday { weekday, .. } => 1 << weekday.rem_euclid(7),
            _ => 127,
        }
    }

    /// Whether the rule itself fires on `date`. The active window (`start_date`/`end_date`)
    /// is applied by the caller — see [`occurs`].
    pub fn occurs_on(&self, date: Date, anchor: Option<Date>) -> bool {
        let anchor = anchor.unwrap_or_else(epoch_monday);
        match self {
            Schedule::Daily { interval } => {
                let n = (*interval).max(1);
                let delta = (date.to_julian_day() - anchor.to_julian_day()) as i64;
                delta.rem_euclid(n) == 0
            }
            Schedule::Weekly { weekdays, interval } => {
                if weekdays & weekday_bit(date) == 0 {
                    return false;
                }
                let n = (*interval).max(1);
                if n == 1 {
                    return true;
                }
                // Compare whole weeks between the two dates' Mondays, so a mask spanning
                // Mon–Sun stays inside the same "every n weeks" slot.
                let weeks = (monday_of(date).to_julian_day() - monday_of(anchor).to_julian_day())
                    as i64
                    / 7;
                weeks.rem_euclid(n) == 0
            }
            Schedule::Monthly { days } => {
                let dom = date.day() as i32;
                let last = days_in_month(date) as i32;
                days.iter().any(|d| if *d == -1 { dom == last } else { *d == dom })
            }
            Schedule::NthWeekday { weekday, nth } => {
                if date.weekday().number_days_from_monday() as i32 != weekday.rem_euclid(7) {
                    return false;
                }
                let dom = date.day() as i32;
                if *nth == -1 {
                    dom + 7 > days_in_month(date) as i32
                } else {
                    (dom - 1) / 7 + 1 == *nth
                }
            }
            Schedule::Once { date: d } => *d == date,
        }
    }
}

fn monday_of(date: Date) -> Date {
    date - time::Duration::days(date.weekday().number_days_from_monday() as i64)
}

fn days_in_month(date: Date) -> u8 {
    time::util::days_in_month(date.month(), date.year())
}

/// Whether a routine is due on `date`: inside its active window and matching its rule.
/// Falls back to the legacy weekday mask when `schedule` is NULL (rows predating 0078).
pub fn occurs(routine: &Routine, date: Date) -> bool {
    if routine.start_date.is_some_and(|s| date < s) || routine.end_date.is_some_and(|e| date > e) {
        return false;
    }
    match &routine.schedule {
        Some(s) => s.0.occurs_on(date, routine.start_date),
        None => routine.weekdays & weekday_bit(date) != 0,
    }
}

/// Mon=0 … Sun=6, matching the mask's bit order.
pub fn weekday_index(w: Weekday) -> i32 {
    w.number_days_from_monday() as i32
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

/// The starter categories, in trading-day order: (name, colour, legacy session key).
pub const DEFAULT_CATEGORIES: &[(&str, &str, &str)] = &[
    ("Pre-market", "var(--chart-1)", "pre"),
    ("In session", "var(--chart-2)", "live"),
    ("Post-market", "var(--chart-3)", "post"),
    ("Post-mortem", "var(--chart-4)", "post"),
    ("Week review", "var(--chart-5)", "any"),
    ("Edge research", "var(--chart-6)", "any"),
];

/// Settings flag marking that the starter routines were installed once. One-shot so a user
/// who deletes them isn't refilled on the next board load.
const SEEDED_KEY: &str = "trader_routines_seeded";

/// Install the starter categories and routines on first use only (never after the user has
/// touched the set).
pub async fn seed_if_first_run(pool: &PgPool) -> anyhow::Result<()> {
    if crate::settings::get(pool, SEEDED_KEY).await?.is_some() {
        // Categories arrived after routines did; an install seeded before 0078 has the
        // flag set but an empty category list, so fill that gap without touching routines.
        ensure_categories(pool).await?;
        return Ok(());
    }
    ensure_categories(pool).await?;
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM trader_routines")
        .fetch_one(pool)
        .await
        .context("counting routines")?;
    if count == 0 {
        let cats = list_categories(pool).await?;
        for (name, session, weekdays, items) in default_routines() {
            let labels: Vec<NewItem> = items
                .into_iter()
                .map(|label| NewItem { label: label.to_owned(), ..NewItem::default() })
                .collect();
            let category_id = cats
                .iter()
                .find(|c| DEFAULT_CATEGORIES.iter().any(|(n, _, s)| *n == c.name && *s == session))
                .map(|c| c.id);
            create_routine(
                pool,
                NewRoutine {
                    name: name.to_owned(),
                    category_id,
                    session: session.to_owned(),
                    schedule: Schedule::Weekly { weekdays, interval: 1 },
                    ..NewRoutine::default()
                },
                &labels,
            )
            .await?;
        }
    }
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

// ── Categories ───────────────────────────────────────────────────────────────

/// Settings flag marking that the starter categories were installed once.
const CATS_SEEDED_KEY: &str = "trader_routine_categories_seeded";

/// Install the starter categories once. Guarded by its own flag rather than by "is the
/// table empty": migration 0078 backfills a category per *session bucket in use*, so an
/// upgraded install starts with three rows and would otherwise never get the buckets the
/// module now ships (post-mortem, week review, edge research). Existing names are skipped,
/// so the backfilled three are matched rather than duplicated, and after this runs once a
/// user who deletes a category is never refilled.
async fn ensure_categories(pool: &PgPool) -> anyhow::Result<()> {
    if crate::settings::get(pool, CATS_SEEDED_KEY).await?.is_some() {
        return Ok(());
    }
    let existing: Vec<String> =
        sqlx::query_scalar("SELECT name FROM trader_routine_categories")
            .fetch_all(pool)
            .await
            .context("listing category names")?;
    // Distinct names only — several starter categories share a legacy session bucket.
    let mut seen: Vec<&str> = Vec::new();
    for (i, (name, color, _)) in DEFAULT_CATEGORIES.iter().enumerate() {
        if seen.contains(name) || existing.iter().any(|e| e == name) {
            continue;
        }
        seen.push(name);
        sqlx::query(
            "INSERT INTO trader_routine_categories (id, name, color, position) VALUES ($1,$2,$3,$4)",
        )
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(color)
        .bind(i as f64)
        .execute(pool)
        .await
        .context("seeding category")?;
    }
    crate::settings::set(pool, CATS_SEEDED_KEY, "1").await?;
    Ok(())
}

pub async fn list_categories(pool: &PgPool) -> anyhow::Result<Vec<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "SELECT id, name, color, position FROM trader_routine_categories \
         ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing categories")?)
}

pub async fn create_category(pool: &PgPool, name: &str, color: &str) -> anyhow::Result<Category> {
    Ok(sqlx::query_as::<_, Category>(
        "INSERT INTO trader_routine_categories (id, name, color, position) \
         VALUES ($1,$2,$3,(SELECT COALESCE(MAX(position),0)+1 FROM trader_routine_categories)) \
         RETURNING id, name, color, position",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(color)
    .fetch_one(pool)
    .await
    .context("creating category")?)
}

pub async fn update_category(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    color: Option<&str>,
    position: Option<f64>,
) -> anyhow::Result<Option<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "UPDATE trader_routine_categories SET \
         name = COALESCE($2, name), color = COALESCE($3, color), position = COALESCE($4, position) \
         WHERE id = $1 RETURNING id, name, color, position",
    )
    .bind(id)
    .bind(name)
    .bind(color)
    .bind(position)
    .fetch_optional(pool)
    .await
    .context("updating category")?)
}

/// Delete a category; its templates survive and fall back to "uncategorised"
/// (`ON DELETE SET NULL`).
pub async fn delete_category(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM trader_routine_categories WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting category")?;
    Ok(())
}

// ── Routines (templates) ─────────────────────────────────────────────────────

const ROUTINE_COLS: &str = "id, name, description, notes, session, category_id, weekdays, \
                            schedule, start_date, end_date, position, active";

/// Everything a template carries on create. `session` stays for the legacy column and the
/// dashboard widget's grouping; the category is the field the UI actually edits.
#[derive(Debug, Clone)]
pub struct NewRoutine {
    pub name: String,
    pub description: String,
    pub notes: String,
    pub session: String,
    pub category_id: Option<Uuid>,
    pub schedule: Schedule,
    pub start_date: Option<Date>,
    pub end_date: Option<Date>,
}

impl Default for NewRoutine {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            notes: String::new(),
            session: "any".into(),
            category_id: None,
            schedule: Schedule::Weekly { weekdays: 31, interval: 1 },
            start_date: None,
            end_date: None,
        }
    }
}

/// A checklist line as the template form submits it. `id` present = keep that row (and its
/// tick history); absent = insert.
#[derive(Debug, Clone, Default)]
pub struct NewItem {
    pub id: Option<Uuid>,
    pub label: String,
    pub note: String,
    pub url: String,
    pub link_label: String,
}

pub async fn list_routines(pool: &PgPool) -> anyhow::Result<Vec<Routine>> {
    let sql = format!(
        "SELECT {ROUTINE_COLS} FROM trader_routines ORDER BY position, created_at"
    );
    Ok(sqlx::query_as::<_, Routine>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing routines")?)
}

pub async fn get_routine(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Routine>> {
    let sql = format!("SELECT {ROUTINE_COLS} FROM trader_routines WHERE id = $1");
    Ok(sqlx::query_as::<_, Routine>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching routine")?)
}

pub async fn create_routine(
    pool: &PgPool,
    r: NewRoutine,
    items: &[NewItem],
) -> anyhow::Result<Routine> {
    let sql = format!(
        "INSERT INTO trader_routines \
         (id, name, description, notes, session, category_id, weekdays, schedule, \
          start_date, end_date, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, \
                 (SELECT COALESCE(MAX(position),0)+1 FROM trader_routines)) \
         RETURNING {ROUTINE_COLS}"
    );
    let routine = sqlx::query_as::<_, Routine>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(&r.name)
        .bind(&r.description)
        .bind(&r.notes)
        .bind(&r.session)
        .bind(r.category_id)
        .bind(r.schedule.weekday_mask())
        .bind(sqlx::types::Json(&r.schedule))
        .bind(r.start_date)
        .bind(r.end_date)
        .fetch_one(pool)
        .await
        .context("creating routine")?;
    set_items(pool, routine.id, items).await?;
    Ok(routine)
}

/// Fields a PATCH may change. `None` leaves a field alone; `Some(None)` on the nullable
/// ones clears them (no end date, no category).
#[derive(Debug, Clone, Default)]
pub struct RoutinePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub session: Option<String>,
    pub category_id: Option<Option<Uuid>>,
    pub schedule: Option<Schedule>,
    pub start_date: Option<Option<Date>>,
    pub end_date: Option<Option<Date>>,
    pub active: Option<bool>,
    pub position: Option<f64>,
}

pub async fn update_routine(
    pool: &PgPool,
    id: Uuid,
    p: RoutinePatch,
) -> anyhow::Result<Option<Routine>> {
    // The weekday mask is derived, never sent: it tracks whatever schedule is written.
    let mask = p.schedule.as_ref().map(Schedule::weekday_mask);
    let sql = format!(
        "UPDATE trader_routines SET \
         name = COALESCE($2, name), \
         description = COALESCE($3, description), \
         notes = COALESCE($4, notes), \
         session = COALESCE($5, session), \
         category_id = CASE WHEN $6 THEN $7 ELSE category_id END, \
         weekdays = COALESCE($8, weekdays), \
         schedule = COALESCE($9, schedule), \
         start_date = CASE WHEN $10 THEN $11 ELSE start_date END, \
         end_date = CASE WHEN $12 THEN $13 ELSE end_date END, \
         active = COALESCE($14, active), \
         position = COALESCE($15, position), \
         updated_at = now() \
         WHERE id = $1 RETURNING {ROUTINE_COLS}"
    );
    Ok(sqlx::query_as::<_, Routine>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(p.name.as_deref())
        .bind(p.description.as_deref())
        .bind(p.notes.as_deref())
        .bind(p.session.as_deref())
        .bind(p.category_id.is_some())
        .bind(p.category_id.flatten())
        .bind(mask)
        .bind(p.schedule.map(sqlx::types::Json))
        .bind(p.start_date.is_some())
        .bind(p.start_date.flatten())
        .bind(p.end_date.is_some())
        .bind(p.end_date.flatten())
        .bind(p.active)
        .bind(p.position)
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

/// Copy a template (items included, tick history excluded) under a new name.
pub async fn duplicate_routine(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<Routine> {
    let src = get_routine(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("routine not found"))?;
    let items: Vec<NewItem> = list_items(pool, id)
        .await?
        .into_iter()
        .map(|i| NewItem {
            id: None,
            label: i.label,
            note: i.note,
            url: i.url,
            link_label: i.link_label,
        })
        .collect();
    create_routine(
        pool,
        NewRoutine {
            name: name.to_owned(),
            description: src.description,
            notes: src.notes,
            session: src.session,
            category_id: src.category_id,
            schedule: src
                .schedule
                .map(|s| s.0)
                .unwrap_or(Schedule::Weekly { weekdays: src.weekdays, interval: 1 }),
            start_date: src.start_date,
            end_date: src.end_date,
        },
        &items,
    )
    .await
}

// ── Routine items ────────────────────────────────────────────────────────────

/// Replace a routine's items with `items` (id-keeping): entries carrying an id update that
/// row's fields/position; entries without one are inserted; existing items not referenced
/// are deleted (with their tick history — they no longer exist).
pub async fn set_items(pool: &PgPool, routine_id: Uuid, items: &[NewItem]) -> anyhow::Result<()> {
    let keep: Vec<Uuid> = items.iter().filter_map(|i| i.id).collect();
    sqlx::query("DELETE FROM trader_routine_items WHERE routine_id = $1 AND NOT (id = ANY($2))")
        .bind(routine_id)
        .bind(&keep)
        .execute(pool)
        .await
        .context("pruning routine items")?;
    for (idx, it) in items.iter().enumerate() {
        match it.id {
            Some(id) => {
                sqlx::query(
                    "UPDATE trader_routine_items \
                     SET label = $2, note = $3, url = $4, link_label = $5, position = $6 \
                     WHERE id = $1 AND routine_id = $7",
                )
                .bind(id)
                .bind(&it.label)
                .bind(&it.note)
                .bind(&it.url)
                .bind(&it.link_label)
                .bind(idx as f64)
                .bind(routine_id)
                .execute(pool)
                .await
                .context("updating routine item")?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO trader_routine_items \
                     (id, routine_id, label, note, url, link_label, position) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7)",
                )
                .bind(Uuid::new_v4())
                .bind(routine_id)
                .bind(&it.label)
                .bind(&it.note)
                .bind(&it.url)
                .bind(&it.link_label)
                .bind(idx as f64)
                .execute(pool)
                .await
                .context("adding routine item")?;
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

/// Active routines due on `date`, with items and their tick state.
///
/// The weekday mask pre-filters in SQL (it is maintained for every schedule kind, and is a
/// superset of what any rule fires on), then [`occurs`] applies the real rule and the active
/// window in Rust. Cheap, and it keeps one evaluator for both the board and the calendar.
pub async fn board_routines(pool: &PgPool, date: Date) -> anyhow::Result<Vec<RoutineView>> {
    let bit = weekday_bit(date);
    let sql = format!(
        "SELECT {ROUTINE_COLS} FROM trader_routines \
         WHERE active AND (weekdays & $1) <> 0 ORDER BY position, created_at"
    );
    let routines = sqlx::query_as::<_, Routine>(sqlx::AssertSqlSafe(sql))
        .bind(bit)
        .fetch_all(pool)
        .await
        .context("listing due routines")?;

    let mut out = Vec::new();
    for r in routines.into_iter().filter(|r| occurs(r, date)) {
        let items = sqlx::query_as::<_, (Uuid, Uuid, String, String, String, String, f64, bool)>(
            "SELECT i.id, i.routine_id, i.label, i.note, i.url, i.link_label, i.position, \
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
        .map(
            |(id, routine_id, label, note, url, link_label, position, checked)| ItemView {
                item: RoutineItem { id, routine_id, label, note, url, link_label, position },
                checked,
            },
        )
        .collect();
        out.push(RoutineView { routine: r, items });
    }
    Ok(out)
}

/// Dates in `[from, to]` on which each active routine is due — feeds the schedule preview
/// calendar in the template editor. Returns (routine id, dates).
pub async fn upcoming(
    pool: &PgPool,
    from: Date,
    to: Date,
) -> anyhow::Result<Vec<(Uuid, Vec<Date>)>> {
    let routines = list_routines(pool).await?;
    let mut out = Vec::new();
    for r in routines.iter().filter(|r| r.active) {
        let mut days = Vec::new();
        let mut d = from;
        while d <= to {
            if occurs(r, d) {
                days.push(d);
            }
            d = d.next_day().unwrap_or(d);
            if d == to && days.len() > 400 {
                break; // guard against a pathological range
            }
        }
        out.push((r.id, days));
    }
    Ok(out)
}

/// Items of a routine (for the manage/edit form).
pub async fn list_items(pool: &PgPool, routine_id: Uuid) -> anyhow::Result<Vec<RoutineItem>> {
    Ok(sqlx::query_as::<_, RoutineItem>(
        "SELECT id, routine_id, label, note, url, link_label, position \
         FROM trader_routine_items WHERE routine_id = $1 ORDER BY position",
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

// ── Day marks (consistency) ──────────────────────────────────────────────────

/// One day's verdict: `full` (acted + followed the routine) or `action` (acted without it).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DayMark {
    #[serde(with = "date_req")]
    pub day: Date,
    pub mark: String,
}

mod date_req {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

pub const DAY_MARKS: &[&str] = &["full", "action"];

pub async fn list_marks(pool: &PgPool, from: Date, to: Date) -> anyhow::Result<Vec<DayMark>> {
    Ok(sqlx::query_as::<_, DayMark>(
        "SELECT day, mark FROM trader_day_marks WHERE day BETWEEN $1 AND $2 ORDER BY day",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .context("listing day marks")?)
}

pub async fn get_mark(pool: &PgPool, day: Date) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>("SELECT mark FROM trader_day_marks WHERE day = $1")
        .bind(day)
        .fetch_optional(pool)
        .await
        .context("fetching day mark")?)
}

/// Set a day's mark, or clear it with `None` (the UI cycles green → amber → nothing).
pub async fn set_mark(pool: &PgPool, day: Date, mark: Option<&str>) -> anyhow::Result<()> {
    match mark {
        Some(m) => {
            sqlx::query(
                "INSERT INTO trader_day_marks (day, mark) VALUES ($1,$2) \
                 ON CONFLICT (day) DO UPDATE SET mark = EXCLUDED.mark, updated_at = now()",
            )
            .bind(day)
            .bind(m)
            .execute(pool)
            .await
            .context("setting day mark")?;
        }
        None => {
            sqlx::query("DELETE FROM trader_day_marks WHERE day = $1")
                .bind(day)
                .execute(pool)
                .await
                .context("clearing day mark")?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::from_calendar_date(y, Month::try_from(m).unwrap(), day).unwrap()
    }

    #[test]
    fn weekly_mask_matches_weekdays_only() {
        let s = Schedule::Weekly { weekdays: 31, interval: 1 }; // Mon–Fri
        assert!(s.occurs_on(d(2026, 8, 13), None)); // Thursday
        assert!(!s.occurs_on(d(2026, 8, 15), None)); // Saturday
        assert!(!s.occurs_on(d(2026, 8, 16), None)); // Sunday
    }

    #[test]
    fn every_thursday_is_a_one_bit_weekly() {
        let s = Schedule::Weekly { weekdays: 8, interval: 1 };
        assert!(s.occurs_on(d(2026, 8, 13), None));
        assert!(!s.occurs_on(d(2026, 8, 14), None));
    }

    #[test]
    fn biweekly_counts_whole_weeks_from_the_anchor() {
        let anchor = Some(d(2026, 8, 10)); // a Monday
        let s = Schedule::Weekly { weekdays: 31, interval: 2 };
        assert!(s.occurs_on(d(2026, 8, 14), anchor)); // same week as the anchor
        assert!(!s.occurs_on(d(2026, 8, 18), anchor)); // week +1 — skipped
        assert!(s.occurs_on(d(2026, 8, 25), anchor)); // week +2
    }

    #[test]
    fn daily_interval_steps_from_the_anchor() {
        let anchor = Some(d(2026, 8, 13));
        let s = Schedule::Daily { interval: 3 };
        assert!(s.occurs_on(d(2026, 8, 13), anchor));
        assert!(!s.occurs_on(d(2026, 8, 14), anchor));
        assert!(s.occurs_on(d(2026, 8, 16), anchor));
    }

    #[test]
    fn monthly_handles_last_day() {
        let s = Schedule::Monthly { days: vec![1, -1] };
        assert!(s.occurs_on(d(2026, 2, 1), None));
        assert!(s.occurs_on(d(2026, 2, 28), None)); // last day of a non-leap February
        assert!(!s.occurs_on(d(2026, 2, 27), None));
    }

    #[test]
    fn nth_weekday_first_and_last() {
        let first_thu = Schedule::NthWeekday { weekday: 3, nth: 1 };
        assert!(first_thu.occurs_on(d(2026, 8, 6), None));
        assert!(!first_thu.occurs_on(d(2026, 8, 13), None));

        let last_fri = Schedule::NthWeekday { weekday: 4, nth: -1 };
        assert!(last_fri.occurs_on(d(2026, 8, 28), None));
        assert!(!last_fri.occurs_on(d(2026, 8, 21), None));
    }

    #[test]
    fn active_window_bounds_the_rule() {
        let mut r = Routine {
            id: Uuid::nil(),
            name: "x".into(),
            description: String::new(),
            notes: String::new(),
            session: "any".into(),
            category_id: None,
            weekdays: 127,
            schedule: Some(sqlx::types::Json(Schedule::Daily { interval: 1 })),
            start_date: Some(d(2026, 8, 10)),
            end_date: Some(d(2026, 8, 20)),
            position: 0.0,
            active: true,
        };
        assert!(!occurs(&r, d(2026, 8, 9)));
        assert!(occurs(&r, d(2026, 8, 15)));
        assert!(!occurs(&r, d(2026, 8, 21)));

        // No schedule at all (rows predating 0078) falls back to the mask.
        r.schedule = None;
        r.weekdays = 8; // Thursdays
        r.start_date = None;
        r.end_date = None;
        assert!(occurs(&r, d(2026, 8, 13)));
        assert!(!occurs(&r, d(2026, 8, 12)));
    }

    #[test]
    fn mask_is_a_superset_of_every_rule() {
        // board_routines pre-filters in SQL on the mask, so a rule must never fire on a
        // weekday its mask excludes.
        for s in [
            Schedule::Daily { interval: 5 },
            Schedule::Monthly { days: vec![1, 15, -1] },
            Schedule::NthWeekday { weekday: 2, nth: 1 },
            Schedule::Once { date: d(2026, 8, 13) },
            Schedule::Weekly { weekdays: 65, interval: 3 },
        ] {
            let mask = s.weekday_mask();
            let mut day = d(2026, 1, 1);
            let end = d(2027, 1, 1);
            while day < end {
                if s.occurs_on(day, Some(d(2026, 1, 1))) {
                    assert!(mask & weekday_bit(day) != 0, "{s:?} fired outside its mask on {day}");
                }
                day = day.next_day().unwrap();
            }
        }
    }
}
