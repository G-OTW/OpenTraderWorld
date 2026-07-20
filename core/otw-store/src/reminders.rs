//! Storage + firing logic for the RemindMe module.
//!
//! A reminder fires in-app notifications on a cadence. It can link to a goal, a todo, or
//! be fully custom. `fire_due` (called by a ~minute tick) finds reminders whose
//! `next_fire_at` is due, writes a `notifications` row, advances `next_fire_at` by the
//! frequency, and increments `fired_count` — stopping at `end_date` or `max_count`.
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime, Time};
use uuid::Uuid;

pub const KINDS: [&str; 3] = ["goal", "todo", "custom"];
pub const FREQUENCIES: [&str; 5] = ["once", "daily", "weekly", "monthly", "yearly"];

// ── Reminders ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Reminder {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub linked_id: Option<Uuid>,
    pub details: String,
    pub frequency: String,
    #[serde(with = "date_iso")]
    pub start_date: Date,
    /// Wall-clock time of day the reminder fires, in the user's local zone (`HH:MM`).
    #[serde(with = "time_hhmm")]
    pub start_time: Time,
    /// User's UTC offset in minutes at save time; maps local `start_time` to a UTC instant.
    pub tz_offset_minutes: i32,
    #[serde(with = "date_opt")]
    pub end_date: Option<Date>,
    pub max_count: Option<i32>,
    pub fired_count: i32,
    #[serde(with = "ts_opt")]
    pub next_fire_at: Option<OffsetDateTime>,
    pub active: bool,
}

mod date_iso {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}
mod time_hhmm {
    use serde::Serializer;
    use time::{format_description::FormatItem, macros::format_description, Time};
    const HHMM: &[FormatItem<'_>] = format_description!("[hour]:[minute]");
    pub fn serialize<S: Serializer>(t: &Time, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(HHMM).map_err(serde::ser::Error::custom)?)
    }
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

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct ReminderInput {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub linked_id: Option<Uuid>,
    #[serde(default)]
    pub details: String,
    #[serde(default = "default_frequency")]
    pub frequency: String,
    /// `YYYY-MM-DD`; defaults to today when omitted.
    pub start_date: Option<String>,
    /// `HH:MM` local wall-clock; defaults to midnight when omitted.
    pub start_time: Option<String>,
    /// User's UTC offset in minutes (e.g. +120 for UTC+2); defaults to 0 (UTC).
    #[serde(default)]
    pub tz_offset_minutes: i32,
    /// `YYYY-MM-DD`, or null/empty for no end.
    pub end_date: Option<String>,
    /// NULL = unlimited.
    pub max_count: Option<i32>,
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_kind() -> String {
    "custom".to_string()
}
fn default_frequency() -> String {
    "once".to_string()
}
fn default_true() -> bool {
    true
}

impl ReminderInput {
    pub fn start(&self) -> Date {
        self.start_date
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
            .unwrap_or_else(|| OffsetDateTime::now_utc().date())
    }
    fn start_time_val(&self) -> Time {
        self.start_time
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(parse_hhmm)
            .unwrap_or(Time::MIDNIGHT)
    }
    fn end(&self) -> Option<Date> {
        self.end_date
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
    }
}

/// Parse an `HH:MM` (or `HH:MM:SS`) clock time.
fn parse_hhmm(s: &str) -> Option<Time> {
    let mut it = s.split(':');
    let h: u8 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    Time::from_hms(h, m, 0).ok()
}

/// First fire instant (UTC) for a reminder: the local wall-clock `start_date @ time`
/// in the user's zone, converted to UTC by subtracting their offset.
fn first_fire(start: Date, time: Time, tz_offset_minutes: i32) -> OffsetDateTime {
    OffsetDateTime::new_utc(start, time) - Duration::minutes(tz_offset_minutes as i64)
}

/// Advance an instant by one period of `frequency`. `once` has no next fire.
fn advance(t: OffsetDateTime, frequency: &str) -> Option<OffsetDateTime> {
    let date = t.date();
    let next = match frequency {
        "daily" => Some(date + Duration::days(1)),
        "weekly" => Some(date + Duration::days(7)),
        "monthly" => Some(add_months(date, 1)),
        "yearly" => Some(add_months(date, 12)),
        _ => None, // once
    }?;
    Some(OffsetDateTime::new_utc(next, t.time()))
}

/// Add `n` months to a date, clamping the day to the target month's length.
fn add_months(d: Date, n: i64) -> Date {
    let total = (d.year() as i64) * 12 + (d.month() as i64 - 1) + n;
    let year = total.div_euclid(12) as i32;
    let month0 = total.rem_euclid(12) as u8;
    let month = time::Month::try_from(month0 + 1).unwrap_or(time::Month::January);
    let day = d.day().min(days_in_month(year, month));
    Date::from_calendar_date(year, month, day).unwrap_or(d)
}

fn days_in_month(year: i32, month: time::Month) -> u8 {
    month.length(year)
}

#[derive(sqlx::FromRow)]
struct ReminderRow {
    id: Uuid,
    name: String,
    kind: String,
    linked_id: Option<Uuid>,
    details: String,
    frequency: String,
    start_date: Date,
    start_time: Time,
    tz_offset_minutes: i32,
    end_date: Option<Date>,
    max_count: Option<i32>,
    fired_count: i32,
    next_fire_at: Option<OffsetDateTime>,
    active: bool,
}

fn row_to_reminder(r: ReminderRow) -> Reminder {
    Reminder {
        id: r.id,
        name: r.name,
        kind: r.kind,
        linked_id: r.linked_id,
        details: r.details,
        frequency: r.frequency,
        start_date: r.start_date,
        start_time: r.start_time,
        tz_offset_minutes: r.tz_offset_minutes,
        end_date: r.end_date,
        max_count: r.max_count,
        fired_count: r.fired_count,
        next_fire_at: r.next_fire_at,
        active: r.active,
    }
}

const COLUMNS: &str = "id, name, kind, linked_id, details, frequency, start_date, start_time, \
                       tz_offset_minutes, end_date, max_count, fired_count, next_fire_at, active";

pub async fn list_reminders(pool: &PgPool) -> anyhow::Result<Vec<Reminder>> {
    let sql = format!("SELECT {COLUMNS} FROM reminders ORDER BY active DESC, next_fire_at NULLS LAST, created_at");
    let rows = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing reminders")?;
    Ok(rows.into_iter().map(row_to_reminder).collect())
}

pub async fn get_reminder(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Reminder>> {
    let sql = format!("SELECT {COLUMNS} FROM reminders WHERE id = $1");
    let row = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching reminder")?;
    Ok(row.map(row_to_reminder))
}

/// How stale a `once` reminder's start may be and still fire on the next tick. Covers a
/// start that fell due while the process was down; anything older was already past when
/// it was written, so firing it would look like a spurious notification.
const ONCE_GRACE: Duration = Duration::hours(1);

/// Compute the initial `next_fire_at` for a (possibly already-started) reminder. If the
/// start is in the past, walk forward to the first not-yet-past occurrence so we don't
/// spam a backlog of notifications.
fn initial_next_fire(input: &ReminderInput) -> Option<OffsetDateTime> {
    if !input.active {
        return None;
    }
    let now = OffsetDateTime::now_utc();
    let mut t = first_fire(input.start(), input.start_time_val(), input.tz_offset_minutes);
    let mut guard = 0;
    while t < now && guard < 10_000 {
        match advance(t, &input.frequency) {
            Some(nt) => t = nt,
            // 'once' in the past: fire only if recently due, else retire unfired.
            None => return (now - t <= ONCE_GRACE).then_some(t),
        }
        guard += 1;
    }
    Some(t)
}

pub async fn add_reminder(pool: &PgPool, input: &ReminderInput) -> anyhow::Result<Reminder> {
    let id = Uuid::new_v4();
    let next = initial_next_fire(input);
    sqlx::query(
        "INSERT INTO reminders \
            (id, name, kind, linked_id, details, frequency, start_date, start_time, \
             tz_offset_minutes, end_date, max_count, fired_count, next_fire_at, active) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,0,$12,$13)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(input.linked_id)
    .bind(&input.details)
    .bind(&input.frequency)
    .bind(input.start())
    .bind(input.start_time_val())
    .bind(input.tz_offset_minutes)
    .bind(input.end())
    .bind(input.max_count)
    .bind(next)
    .bind(input.active)
    .execute(pool)
    .await
    .context("inserting reminder")?;
    get_reminder(pool, id).await?.context("reminder vanished after insert")
}

pub async fn update_reminder(pool: &PgPool, id: Uuid, input: &ReminderInput) -> anyhow::Result<bool> {
    // Recompute the schedule from the (possibly changed) cadence/dates, resetting the count.
    let next = initial_next_fire(input);
    let res = sqlx::query(
        "UPDATE reminders SET \
            name = $2, kind = $3, linked_id = $4, details = $5, frequency = $6, \
            start_date = $7, start_time = $8, tz_offset_minutes = $9, end_date = $10, \
            max_count = $11, fired_count = 0, next_fire_at = $12, active = $13, \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(input.linked_id)
    .bind(&input.details)
    .bind(&input.frequency)
    .bind(input.start())
    .bind(input.start_time_val())
    .bind(input.tz_offset_minutes)
    .bind(input.end())
    .bind(input.max_count)
    .bind(next)
    .bind(input.active)
    .execute(pool)
    .await
    .context("updating reminder")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_reminder(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM reminders WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Firing (background tick) ─────────────────────────────────────────────────

/// A notification just written by [`fire_due`], returned so the caller can also push it
/// to external channels (email/telegram/…). Carries only what a message needs.
#[derive(Debug, Clone)]
pub struct FiredNotification {
    pub name: String,
    pub details: String,
}

/// Fire every reminder whose `next_fire_at` is due. For each: insert a notification,
/// advance the schedule, and increment the count — clearing `next_fire_at` once the
/// reminder is exhausted (`once`, past `end_date`, or `max_count` reached).
/// Returns the notifications created (for both counting and external dispatch).
pub async fn fire_due(pool: &PgPool) -> anyhow::Result<Vec<FiredNotification>> {
    let now = OffsetDateTime::now_utc();
    let sql = format!(
        "SELECT {COLUMNS} FROM reminders \
         WHERE active AND next_fire_at IS NOT NULL AND next_fire_at <= $1"
    );
    let due: Vec<ReminderRow> = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .fetch_all(pool)
        .await
        .context("loading due reminders")?;

    let mut fired: Vec<FiredNotification> = Vec::new();
    for r in due {
        // Create the notification.
        sqlx::query(
            "INSERT INTO notifications (id, reminder_id, name, kind, linked_id, details) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(r.id)
        .bind(&r.name)
        .bind(&r.kind)
        .bind(r.linked_id)
        .bind(&r.details)
        .execute(pool)
        .await
        .context("inserting notification")?;
        fired.push(FiredNotification {
            name: r.name.clone(),
            details: r.details.clone(),
        });

        let new_count = r.fired_count + 1;
        // Compute the next fire, honouring max_count and end_date.
        let mut next = if r.max_count.is_some_and(|m| new_count >= m) {
            None
        } else if let Some(cur) = r.next_fire_at {
            advance(cur, &r.frequency)
        } else {
            None
        };
        if let (Some(nt), Some(end)) = (next, r.end_date) {
            if nt.date() > end {
                next = None;
            }
        }

        sqlx::query(
            "UPDATE reminders SET fired_count = $2, next_fire_at = $3, updated_at = now() \
             WHERE id = $1",
        )
        .bind(r.id)
        .bind(new_count)
        .bind(next)
        .execute(pool)
        .await
        .context("advancing reminder")?;
    }
    Ok(fired)
}

/// Insert a standalone notification (no backing reminder) — e.g. an inbound webhook
/// redirected to RemindMe. Returns it as a [`FiredNotification`] so the caller can also
/// push it to the external channels.
pub async fn add_notification(
    pool: &PgPool,
    name: &str,
    details: &str,
) -> anyhow::Result<FiredNotification> {
    sqlx::query(
        "INSERT INTO notifications (id, reminder_id, name, kind, linked_id, details) \
         VALUES ($1, NULL, $2, 'custom', NULL, $3)",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(details)
    .execute(pool)
    .await
    .context("inserting webhook notification")?;
    Ok(FiredNotification { name: name.to_string(), details: details.to_string() })
}

// ── Notifications ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub reminder_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub linked_id: Option<Uuid>,
    pub details: String,
    #[serde(with = "ts_opt")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(with = "ts_iso")]
    pub created_at: OffsetDateTime,
}

mod ts_iso {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

#[derive(sqlx::FromRow)]
struct NotifRow {
    id: Uuid,
    reminder_id: Option<Uuid>,
    name: String,
    kind: String,
    linked_id: Option<Uuid>,
    details: String,
    read_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

fn row_to_notif(r: NotifRow) -> Notification {
    Notification {
        id: r.id,
        reminder_id: r.reminder_id,
        name: r.name,
        kind: r.kind,
        linked_id: r.linked_id,
        details: r.details,
        read_at: r.read_at,
        created_at: r.created_at,
    }
}

const NOTIF_COLUMNS: &str =
    "id, reminder_id, name, kind, linked_id, details, read_at, created_at";

pub async fn list_notifications(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Notification>> {
    let sql = format!(
        "SELECT {NOTIF_COLUMNS} FROM notifications ORDER BY created_at DESC LIMIT $1"
    );
    let rows = sqlx::query_as::<_, NotifRow>(sqlx::AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("listing notifications")?;
    Ok(rows.into_iter().map(row_to_notif).collect())
}

/// Unread notifications created strictly after `since` (for the slide-in toast poller).
/// When `since` is None, returns all unread.
pub async fn unread_since(
    pool: &PgPool,
    since: Option<OffsetDateTime>,
) -> anyhow::Result<Vec<Notification>> {
    let sql = format!(
        "SELECT {NOTIF_COLUMNS} FROM notifications \
         WHERE read_at IS NULL AND ($1::timestamptz IS NULL OR created_at > $1) \
         ORDER BY created_at"
    );
    let rows = sqlx::query_as::<_, NotifRow>(sqlx::AssertSqlSafe(sql))
        .bind(since)
        .fetch_all(pool)
        .await
        .context("polling unread notifications")?;
    Ok(rows.into_iter().map(row_to_notif).collect())
}

pub async fn unread_count(pool: &PgPool) -> anyhow::Result<i64> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM notifications WHERE read_at IS NULL")
            .fetch_one(pool)
            .await
            .context("counting unread notifications")?;
    Ok(row.0)
}

/// Mark one notification read. Returns false if it didn't exist.
pub async fn mark_read(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE notifications SET read_at = now() WHERE id = $1 AND read_at IS NULL")
        .bind(id)
        .execute(pool)
        .await
        .context("marking notification read")?;
    Ok(res.rows_affected() > 0)
}

/// Mark all unread notifications read ("acknowledge all"). Returns how many were cleared.
pub async fn mark_all_read(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("UPDATE notifications SET read_at = now() WHERE read_at IS NULL")
        .execute(pool)
        .await
        .context("acknowledging all notifications")?;
    Ok(res.rows_affected())
}

pub async fn delete_notification(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM notifications WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
