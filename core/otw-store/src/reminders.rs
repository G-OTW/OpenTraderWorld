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
pub const FREQUENCIES: [&str; 6] = ["once", "interval", "daily", "weekly", "monthly", "yearly"];

/// Longest `interval` period: one week.
pub const MAX_EVERY_MINUTES: i32 = 10_080;
/// Repeat within the day: the whole run (first to last fire) stays under a day.
pub const DAY_MINUTES: i32 = 1_440;

// ── Reminders ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Reminder {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub linked_id: Option<Uuid>,
    pub details: String,
    /// Optional link the reminder is about (http/https), with its own label.
    pub url: String,
    pub link_label: String,
    pub frequency: String,
    /// Weekly only: bitmask of the days it fires on (Mon = 1 … Sun = 64). None/0 = every
    /// 7 days from `start_date`.
    pub weekdays: Option<i16>,
    /// Interval only: the period in minutes.
    pub every_minutes: Option<i32>,
    /// Repeat within the day: minutes between fires, and fires per day (first included).
    pub repeat_every_minutes: Option<i32>,
    pub repeat_count: Option<i32>,
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
    /// Optional `http(s)://` link shown with the reminder and its notifications.
    #[serde(default)]
    pub url: String,
    /// Text for that link; falls back to the URL itself when empty.
    #[serde(default)]
    pub link_label: String,
    #[serde(default = "default_frequency")]
    pub frequency: String,
    /// Weekly only: bitmask of the days to fire on (Mon = 1 … Sun = 64, 127 = every day).
    /// Omit for the plain "every 7 days from the start date" cadence.
    pub weekdays: Option<i16>,
    /// Interval only: fire every this many minutes (1..=10080) from the start date and time.
    pub every_minutes: Option<i32>,
    /// Any other frequency: on each day it fires, fire again every this many minutes after
    /// `start_time`. Set together with `repeat_count`; omit both for one fire per day.
    pub repeat_every_minutes: Option<i32>,
    /// Fires per day, the first one at `start_time` included (2 or more).
    pub repeat_count: Option<i32>,
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
    fn burst(&self) -> Option<Burst> {
        Burst::of(&self.frequency, self.repeat_every_minutes, self.repeat_count)
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

/// Does `d` fall on one of the days set in a weekday mask (Mon = 1 … Sun = 64)?
fn mask_has(mask: i16, d: Date) -> bool {
    mask & (1 << d.weekday().number_days_from_monday()) != 0
}

/// The period of an `interval` reminder, or None for any other frequency.
fn interval_step(frequency: &str, every_minutes: Option<i32>) -> Option<Duration> {
    (frequency == "interval")
        .then_some(every_minutes)
        .flatten()
        .filter(|m| *m > 0)
        .map(|m| Duration::minutes(m as i64))
}

/// First occurrence of an interval schedule anchored at `t` that is not before `now`.
fn interval_catch_up(t: OffsetDateTime, step: Duration, now: OffsetDateTime) -> OffsetDateTime {
    if t >= now {
        return t;
    }
    let behind = (now - t).whole_seconds();
    let step_s = step.whole_seconds();
    let k = (behind + step_s - 1) / step_s;
    t + Duration::seconds(k * step_s)
}

/// Repeat within the day: `count` fires, `step` apart, from the day's start time.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Burst {
    step: Duration,
    count: i32,
}

impl Burst {
    fn of(frequency: &str, every: Option<i32>, count: Option<i32>) -> Option<Self> {
        if frequency == "interval" {
            return None;
        }
        let every = every.filter(|m| *m > 0)?;
        let count = count.filter(|c| *c > 1)?;
        Some(Burst { step: Duration::minutes(every as i64), count })
    }
    /// From the day's first fire to its last.
    fn span(&self) -> Duration {
        self.step * (self.count - 1)
    }
}

/// When and how a reminder repeats: what the next fire is computed from.
struct Sched<'a> {
    frequency: &'a str,
    weekdays: Option<i16>,
    every_minutes: Option<i32>,
    burst: Option<Burst>,
    start_time: Time,
    tz_offset_minutes: i32,
}

impl Sched<'_> {
    /// The first fire of the day a fire at `t` belongs to: the latest instant at the local
    /// start time not after `t` (a run that crosses midnight belongs to the day before).
    fn day_start(&self, t: OffsetDateTime) -> OffsetDateTime {
        let off = Duration::minutes(self.tz_offset_minutes as i64);
        let local = t + off;
        let mut first = OffsetDateTime::new_utc(local.date(), self.start_time);
        if first > local {
            first -= Duration::days(1);
        }
        first - off
    }

    /// The fire after `cur`: the next one of the same day while the day's run lasts, else
    /// the first one of the next day the schedule fires on.
    fn next(&self, cur: OffsetDateTime) -> Option<OffsetDateTime> {
        if let Some(b) = self.burst {
            let first = self.day_start(cur);
            let done = (cur - first).whole_minutes() / b.step.whole_minutes() + 1;
            if done < b.count as i64 {
                return Some(cur + b.step);
            }
            return advance(first, self.frequency, self.weekdays, None, self.tz_offset_minutes);
        }
        advance(cur, self.frequency, self.weekdays, self.every_minutes, self.tz_offset_minutes)
    }

    /// The day an occurrence counts for, checked against `end_date`.
    fn occurrence_date(&self, t: OffsetDateTime) -> Date {
        match self.burst {
            Some(_) => self.day_start(t).date(),
            None => t.date(),
        }
    }
}

/// A usable weekday mask, or None when the reminder uses the plain 7-day cadence.
fn week_mask(frequency: &str, weekdays: Option<i16>) -> Option<i16> {
    (frequency == "weekly")
        .then_some(weekdays)
        .flatten()
        .filter(|m| *m & 0b111_1111 != 0)
}

/// Advance an instant by one period of `frequency`. `once` has no next fire. A weekly
/// reminder with a day mask hops to the next selected day instead of adding 7 days —
/// the mask is matched against the *local* date, so `tz_offset_minutes` is applied first.
fn advance(
    t: OffsetDateTime,
    frequency: &str,
    weekdays: Option<i16>,
    every_minutes: Option<i32>,
    tz_offset_minutes: i32,
) -> Option<OffsetDateTime> {
    if frequency == "interval" {
        return interval_step(frequency, every_minutes).map(|step| t + step);
    }
    if let Some(mask) = week_mask(frequency, weekdays) {
        let off = Duration::minutes(tz_offset_minutes as i64);
        let local = t + off;
        let mut d = local.date();
        for _ in 0..7 {
            d += Duration::days(1);
            if mask_has(mask, d) {
                return Some(OffsetDateTime::new_utc(d, local.time()) - off);
            }
        }
        return None;
    }
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
    url: String,
    link_label: String,
    frequency: String,
    weekdays: Option<i16>,
    every_minutes: Option<i32>,
    repeat_every_minutes: Option<i32>,
    repeat_count: Option<i32>,
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
        url: r.url,
        link_label: r.link_label,
        frequency: r.frequency,
        weekdays: r.weekdays,
        every_minutes: r.every_minutes,
        repeat_every_minutes: r.repeat_every_minutes,
        repeat_count: r.repeat_count,
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

const COLUMNS: &str = "id, name, kind, linked_id, details, url, link_label, frequency, weekdays, \
                       every_minutes, repeat_every_minutes, repeat_count, start_date, start_time, tz_offset_minutes, end_date, max_count, \
                       fired_count, next_fire_at, active";

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
    let mut start = input.start();
    // A day mask that excludes the start day: begin on the first selected day instead, so
    // "every Monday" starting on a Wednesday doesn't fire that Wednesday.
    if let Some(mask) = week_mask(&input.frequency, input.weekdays) {
        for _ in 0..7 {
            if mask_has(mask, start) {
                break;
            }
            start += Duration::days(1);
        }
    }
    let mut t = first_fire(start, input.start_time_val(), input.tz_offset_minutes);
    // Minute-level periods can be far behind: jump straight to the next slot.
    if let Some(step) = interval_step(&input.frequency, input.every_minutes) {
        return Some(interval_catch_up(t, step, now));
    }
    // Walk whole days (the day's first fire) until one still has a fire ahead, then take the
    // first of that day's fires not yet past.
    let burst = input.burst();
    let span = burst.map(|b| b.span()).unwrap_or(Duration::ZERO);
    let mut guard = 0;
    while t + span < now && guard < 10_000 {
        match advance(t, &input.frequency, input.weekdays, None, input.tz_offset_minutes) {
            Some(nt) => t = nt,
            // 'once' in the past: fire only if recently due, else retire unfired.
            None => return (now - (t + span) <= ONCE_GRACE).then_some(t + span),
        }
        guard += 1;
    }
    if let Some(b) = burst {
        t = interval_catch_up(t, b.step, now);
    }
    Some(t)
}

pub async fn add_reminder(pool: &PgPool, input: &ReminderInput) -> anyhow::Result<Reminder> {
    let id = Uuid::new_v4();
    let next = initial_next_fire(input);
    sqlx::query(
        "INSERT INTO reminders \
            (id, name, kind, linked_id, details, url, link_label, frequency, weekdays, \
             start_date, start_time, tz_offset_minutes, end_date, max_count, fired_count, \
             next_fire_at, active, every_minutes, repeat_every_minutes, repeat_count) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,0,$15,$16,$17,$18,$19)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(input.linked_id)
    .bind(&input.details)
    .bind(&input.url)
    .bind(&input.link_label)
    .bind(&input.frequency)
    .bind(week_mask(&input.frequency, input.weekdays))
    .bind(input.start())
    .bind(input.start_time_val())
    .bind(input.tz_offset_minutes)
    .bind(input.end())
    .bind(input.max_count)
    .bind(next)
    .bind(input.active)
    .bind(interval_step(&input.frequency, input.every_minutes).and(input.every_minutes))
    .bind(input.burst().and(input.repeat_every_minutes))
    .bind(input.burst().and(input.repeat_count))
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
            name = $2, kind = $3, linked_id = $4, details = $5, url = $6, link_label = $7, \
            frequency = $8, weekdays = $9, start_date = $10, start_time = $11, \
            tz_offset_minutes = $12, end_date = $13, max_count = $14, fired_count = 0, \
            next_fire_at = $15, active = $16, every_minutes = $17, \
            repeat_every_minutes = $18, repeat_count = $19, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(input.linked_id)
    .bind(&input.details)
    .bind(&input.url)
    .bind(&input.link_label)
    .bind(&input.frequency)
    .bind(week_mask(&input.frequency, input.weekdays))
    .bind(input.start())
    .bind(input.start_time_val())
    .bind(input.tz_offset_minutes)
    .bind(input.end())
    .bind(input.max_count)
    .bind(next)
    .bind(input.active)
    .bind(interval_step(&input.frequency, input.every_minutes).and(input.every_minutes))
    .bind(input.burst().and(input.repeat_every_minutes))
    .bind(input.burst().and(input.repeat_count))
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
    /// The reminder's link, appended to the outgoing message when set.
    pub url: String,
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
            "INSERT INTO notifications \
                (id, reminder_id, name, kind, linked_id, details, url, link_label) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(Uuid::new_v4())
        .bind(r.id)
        .bind(&r.name)
        .bind(&r.kind)
        .bind(r.linked_id)
        .bind(&r.details)
        .bind(&r.url)
        .bind(&r.link_label)
        .execute(pool)
        .await
        .context("inserting notification")?;
        fired.push(FiredNotification {
            name: r.name.clone(),
            details: r.details.clone(),
            url: r.url.clone(),
        });

        let new_count = r.fired_count + 1;
        let sched = Sched {
            frequency: &r.frequency,
            weekdays: r.weekdays,
            every_minutes: r.every_minutes,
            burst: Burst::of(&r.frequency, r.repeat_every_minutes, r.repeat_count),
            start_time: r.start_time,
            tz_offset_minutes: r.tz_offset_minutes,
        };
        // Compute the next fire, honouring max_count and end_date.
        let mut next = if r.max_count.is_some_and(|m| new_count >= m) {
            None
        } else if let Some(cur) = r.next_fire_at {
            match interval_step(&r.frequency, r.every_minutes) {
                // Fire once for a backlog (process was down), then resume on the grid.
                Some(step) => Some(interval_catch_up(cur + step, step, now)),
                None => {
                    let mut nt = sched.next(cur);
                    // Same for the runs within a day: missed fires are skipped, not replayed.
                    if sched.burst.is_some() {
                        let mut guard = 0;
                        while let Some(t) = nt.filter(|t| *t < now && guard < 10_000) {
                            nt = sched.next(t);
                            guard += 1;
                        }
                    }
                    nt
                }
            }
        } else {
            None
        };
        if let (Some(nt), Some(end)) = (next, r.end_date) {
            if sched.occurrence_date(nt) > end {
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
    Ok(FiredNotification {
        name: name.to_string(),
        details: details.to_string(),
        url: String::new(),
    })
}

/// An in-app notification that points somewhere in the app (`url`), under its own `kind`
/// so the bell knows where a click goes.
pub async fn add_linked_notification(
    pool: &PgPool,
    kind: &str,
    name: &str,
    details: &str,
    url: &str,
) -> anyhow::Result<FiredNotification> {
    sqlx::query(
        "INSERT INTO notifications (id, reminder_id, name, kind, linked_id, details, url) \
         VALUES ($1, NULL, $2, $3, NULL, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(kind)
    .bind(details)
    .bind(url)
    .execute(pool)
    .await
    .context("inserting notification")?;
    Ok(FiredNotification {
        name: name.to_string(),
        details: details.to_string(),
        url: url.to_string(),
    })
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
    /// Copied from the reminder at fire time, so it survives the reminder's deletion.
    pub url: String,
    pub link_label: String,
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
    url: String,
    link_label: String,
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
        url: r.url,
        link_label: r.link_label,
        read_at: r.read_at,
        created_at: r.created_at,
    }
}

const NOTIF_COLUMNS: &str =
    "id, reminder_id, name, kind, linked_id, details, url, link_label, read_at, created_at";

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

/// One notification by id (the push stream reloads the row a `NOTIFY` names).
pub async fn get_notification(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Notification>> {
    let sql = format!("SELECT {NOTIF_COLUMNS} FROM notifications WHERE id = $1");
    let row = sqlx::query_as::<_, NotifRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading notification")?;
    Ok(row.map(row_to_notif))
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

/// Delete every notification ("clear all"). Returns how many were removed.
pub async fn delete_all_notifications(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM notifications")
        .execute(pool)
        .await
        .context("clearing all notifications")?;
    Ok(res.rows_affected())
}

pub async fn delete_notification(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM notifications WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn interval_advances_by_its_period() {
        let t = datetime!(2026-11-19 09:00 UTC);
        assert_eq!(advance(t, "interval", None, Some(90), 0), Some(datetime!(2026-11-19 10:30 UTC)));
        assert_eq!(advance(t, "interval", None, None, 0), None);
    }

    fn tue_thu(burst: Option<Burst>) -> Sched<'static> {
        Sched {
            frequency: "weekly",
            weekdays: Some(2 | 8),
            every_minutes: None,
            burst,
            start_time: time::macros::time!(10:00),
            tz_offset_minutes: 120,
        }
    }

    #[test]
    fn a_day_run_fires_count_times_then_moves_to_the_next_day() {
        // Tue and Thu, 10:00 local (UTC+2), then every 10 min, 3 fires a day.
        let s = tue_thu(Burst::of("weekly", Some(10), Some(3)));
        let tue = datetime!(2026-11-17 08:00 UTC);
        assert_eq!(s.next(tue), Some(datetime!(2026-11-17 08:10 UTC)));
        assert_eq!(s.next(datetime!(2026-11-17 08:10 UTC)), Some(datetime!(2026-11-17 08:20 UTC)));
        assert_eq!(s.next(datetime!(2026-11-17 08:20 UTC)), Some(datetime!(2026-11-19 08:00 UTC)));
    }

    #[test]
    fn a_run_crossing_midnight_belongs_to_its_first_day() {
        let mut s = tue_thu(Burst::of("weekly", Some(60), Some(4)));
        s.start_time = time::macros::time!(23:00);
        s.tz_offset_minutes = 0;
        let wed_01 = datetime!(2026-11-18 01:00 UTC);
        assert_eq!(s.day_start(wed_01), datetime!(2026-11-17 23:00 UTC));
        assert_eq!(s.next(wed_01), Some(datetime!(2026-11-18 02:00 UTC)));
        assert_eq!(s.next(datetime!(2026-11-18 02:00 UTC)), Some(datetime!(2026-11-19 23:00 UTC)));
    }

    #[test]
    fn no_burst_keeps_one_fire_a_day() {
        assert_eq!(Burst::of("weekly", Some(10), Some(1)), None);
        assert_eq!(Burst::of("interval", Some(10), Some(3)), None);
        let s = tue_thu(None);
        assert_eq!(s.next(datetime!(2026-11-17 08:00 UTC)), Some(datetime!(2026-11-19 08:00 UTC)));
    }

    #[test]
    fn interval_catch_up_lands_on_the_grid() {
        let step = Duration::minutes(15);
        let start = datetime!(2026-11-19 09:00 UTC);
        assert_eq!(interval_catch_up(start, step, datetime!(2026-11-19 09:31 UTC)), datetime!(2026-11-19 09:45 UTC));
        assert_eq!(interval_catch_up(start, step, datetime!(2026-11-19 09:45 UTC)), datetime!(2026-11-19 09:45 UTC));
        assert_eq!(interval_catch_up(start, step, datetime!(2026-11-19 08:00 UTC)), start);
    }
}
