//! Storage + analytics for the Time Tracker module.
//!
//! Projects hold an optional time budget (hours) for alerting and an optional hourly rate
//! to value tracked time. A running timer is a `time_entries` row with `ended_at IS NULL`;
//! elapsed is computed as `now() - started_at`, so timers survive a browser close. The
//! client heartbeats `last_seen_at`; on reopen a still-running timer can be reverted back to
//! that timestamp (discarding time accrued while away). Single-user.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

// ── Projects ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub category: Option<String>,
    pub color: Option<String>,
    #[serde(with = "date_opt")]
    pub planned_end: Option<Date>,
    pub time_budget_hours: Option<f64>,
    pub hourly_rate: Option<f64>,
    pub rate_currency: String,
    pub archived: bool,
    pub position: f64,
    /// Seconds tracked across all closed entries plus any currently-running entry.
    pub tracked_seconds: f64,
    /// Whether a timer is currently running for this project.
    pub running: bool,
    /// When the running timer started (RFC3339), if running — lets the client tick locally.
    #[serde(with = "time::serde::rfc3339::option")]
    pub running_since: Option<OffsetDateTime>,
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

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
pub struct ProjectInput {
    #[serde(default)]
    pub name: String,
    pub category: Option<String>,
    pub color: Option<String>,
    /// `YYYY-MM-DD` or null.
    pub planned_end: Option<String>,
    pub time_budget_hours: Option<f64>,
    pub hourly_rate: Option<f64>,
    #[serde(default = "default_currency")]
    pub rate_currency: String,
    #[serde(default)]
    pub archived: bool,
}

fn default_currency() -> String {
    "USD".to_string()
}

impl ProjectInput {
    fn planned_date(&self) -> Option<Date> {
        self.planned_end
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| crate::journal_fx::parse_date(s).ok())
    }
}

/// List projects with their tracked time (closed + running) computed in SQL.
pub async fn list_projects(pool: &PgPool, include_archived: bool) -> anyhow::Result<Vec<Project>> {
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            Option<String>,
            Option<Date>,
            Option<f64>,
            Option<f64>,
            String,
            bool,
            f64,
            Option<f64>, // tracked_seconds (COALESCE'd but sqlx sees it as nullable)
            Option<OffsetDateTime>,
        ),
    >(
        "SELECT p.id, p.name, p.category, p.color, p.planned_end, p.time_budget_hours, \
                p.hourly_rate, p.rate_currency, p.archived, p.position, \
                COALESCE(SUM(EXTRACT(EPOCH FROM (COALESCE(e.ended_at, now()) - e.started_at))), 0)::double precision \
                    AS tracked_seconds, \
                MAX(CASE WHEN e.ended_at IS NULL THEN e.started_at END) AS running_since \
         FROM time_projects p \
         LEFT JOIN time_entries e ON e.project_id = p.id \
         WHERE ($1 OR NOT p.archived) \
         GROUP BY p.id \
         ORDER BY p.position, p.created_at",
    )
    .bind(include_archived)
    .fetch_all(pool)
    .await
    .context("listing time projects")?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                category,
                color,
                planned_end,
                time_budget_hours,
                hourly_rate,
                rate_currency,
                archived,
                position,
                tracked_seconds,
                running_since,
            )| Project {
                // tracked_seconds is COALESCE'd to 0 in SQL; default just in case.
                id,
                name,
                category,
                color,
                planned_end,
                time_budget_hours,
                hourly_rate,
                rate_currency,
                archived,
                position,
                tracked_seconds: tracked_seconds.unwrap_or(0.0),
                running: running_since.is_some(),
                running_since,
            },
        )
        .collect())
}

pub async fn add_project(pool: &PgPool, input: &ProjectInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM time_projects")
        .fetch_one(pool)
        .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO time_projects \
            (id, name, category, color, planned_end, time_budget_hours, hourly_rate, \
             rate_currency, archived, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.category.as_deref())
    .bind(input.color.as_deref())
    .bind(input.planned_date())
    .bind(input.time_budget_hours)
    .bind(input.hourly_rate)
    .bind(&input.rate_currency)
    .bind(input.archived)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting time project")?;
    Ok(id)
}

pub async fn update_project(pool: &PgPool, id: Uuid, input: &ProjectInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE time_projects SET \
            name = $2, category = $3, color = $4, planned_end = $5, time_budget_hours = $6, \
            hourly_rate = $7, rate_currency = $8, archived = $9, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.category.as_deref())
    .bind(input.color.as_deref())
    .bind(input.planned_date())
    .bind(input.time_budget_hours)
    .bind(input.hourly_rate)
    .bind(&input.rate_currency)
    .bind(input.archived)
    .execute(pool)
    .await
    .context("updating time project")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_project(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM time_projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn set_position(pool: &PgPool, id: Uuid, position: f64) -> anyhow::Result<()> {
    sqlx::query("UPDATE time_projects SET position = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(position)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Timer control ────────────────────────────────────────────────────────────

/// Start a timer for a project. No-op (returns existing) if one is already running, thanks
/// to the partial unique index; we check first to keep it idempotent and quiet.
pub async fn start_timer(pool: &PgPool, project_id: Uuid) -> anyhow::Result<()> {
    let open: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM time_entries WHERE project_id = $1 AND ended_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    if open.is_some() {
        return Ok(());
    }
    sqlx::query("INSERT INTO time_entries (id, project_id, started_at) VALUES ($1, $2, now())")
        .bind(Uuid::new_v4())
        .bind(project_id)
        .execute(pool)
        .await
        .context("starting timer")?;
    Ok(())
}

/// Stop the running timer for a project (sets ended_at = now()). Returns false if none open.
pub async fn stop_timer(pool: &PgPool, project_id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE time_entries SET ended_at = now() \
         WHERE project_id = $1 AND ended_at IS NULL",
    )
    .bind(project_id)
    .execute(pool)
    .await
    .context("stopping timer")?;
    Ok(res.rows_affected() > 0)
}

/// Revert a running timer back to `last_seen_at`: close the open entry at that timestamp,
/// discarding time accrued while the browser was closed. If the entry started after
/// last_seen_at (i.e. it began this session), it's left running. Returns true if reverted.
pub async fn revert_running_to_last_seen(pool: &PgPool) -> anyhow::Result<u64> {
    let last_seen = get_last_seen(pool).await?;
    let res = sqlx::query(
        "UPDATE time_entries SET ended_at = $1 \
         WHERE ended_at IS NULL AND started_at < $1",
    )
    .bind(last_seen)
    .execute(pool)
    .await
    .context("reverting running timers")?;
    Ok(res.rows_affected())
}

/// Whether any timer is currently running (for the reopen prompt).
pub async fn any_running(pool: &PgPool) -> anyhow::Result<bool> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM time_entries WHERE ended_at IS NULL)")
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

// ── Entries ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Entry {
    pub id: Uuid,
    pub project_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub note: Option<String>,
}

/// Runs of a project, newest first. `since`/`until` are inclusive day bounds on the run's
/// start — the same window the breakdown filters on, so a drill-down shows exactly the runs
/// behind a bucket.
pub async fn list_entries(
    pool: &PgPool,
    project_id: Uuid,
    limit: i64,
    since: Option<Date>,
    until: Option<Date>,
) -> anyhow::Result<Vec<Entry>> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, OffsetDateTime, Option<OffsetDateTime>, Option<String>)>(
        "SELECT id, project_id, started_at, ended_at, note FROM time_entries \
         WHERE project_id = $1 \
           AND ($3::date IS NULL OR started_at >= $3) \
           AND ($4::date IS NULL OR started_at < ($4::date + INTERVAL '1 day')) \
         ORDER BY started_at DESC LIMIT $2",
    )
    .bind(project_id)
    .bind(limit)
    .bind(since)
    .bind(until)
    .fetch_all(pool)
    .await
    .context("listing entries")?;
    Ok(rows
        .into_iter()
        .map(|(id, project_id, started_at, ended_at, note)| Entry {
            id,
            project_id,
            started_at,
            ended_at,
            note,
        })
        .collect())
}

pub async fn delete_entry(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM time_entries WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Manually insert a closed time range for a project. Both timestamps are stored as-is
/// (RFC3339, with offset). The caller validates start < end and that the project exists.
pub async fn create_entry(
    pool: &PgPool,
    project_id: Uuid,
    started_at: OffsetDateTime,
    ended_at: OffsetDateTime,
    note: Option<&str>,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO time_entries (id, project_id, started_at, ended_at, note) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(project_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(note)
    .execute(pool)
    .await
    .context("inserting manual time entry")?;
    Ok(id)
}

/// Whether a project exists (for validating manual-entry inserts).
pub async fn project_exists(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let row: (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM time_projects WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

// ── State (heartbeat + settings) ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TimeState {
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub display_currency: String,
    pub any_running: bool,
}

pub async fn get_last_seen(pool: &PgPool) -> anyhow::Result<OffsetDateTime> {
    let row: Option<(OffsetDateTime,)> =
        sqlx::query_as("SELECT last_seen_at FROM time_state WHERE id = TRUE")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0).unwrap_or_else(OffsetDateTime::now_utc))
}

pub async fn get_state(pool: &PgPool) -> anyhow::Result<TimeState> {
    let row: Option<(OffsetDateTime, String)> =
        sqlx::query_as("SELECT last_seen_at, display_currency FROM time_state WHERE id = TRUE")
            .fetch_optional(pool)
            .await
            .context("loading time state")?;
    let (last_seen_at, display_currency) =
        row.unwrap_or_else(|| (OffsetDateTime::now_utc(), "USD".to_string()));
    Ok(TimeState {
        last_seen_at,
        display_currency,
        any_running: any_running(pool).await?,
    })
}

/// Update the heartbeat to now(). Returns the previous last_seen (useful for the client to
/// know the gap). Called periodically while the app is open and on load.
pub async fn heartbeat(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO time_state (id, last_seen_at) VALUES (TRUE, now()) \
         ON CONFLICT (id) DO UPDATE SET last_seen_at = now()",
    )
    .execute(pool)
    .await
    .context("heartbeat")?;
    Ok(())
}

pub async fn set_display_currency(pool: &PgPool, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO time_state (id, display_currency) VALUES (TRUE, $1) \
         ON CONFLICT (id) DO UPDATE SET display_currency = $1",
    )
    .bind(currency)
    .execute(pool)
    .await
    .context("setting time display currency")?;
    Ok(())
}

// ── Breakdown ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BreakdownQuery {
    pub project_id: Option<Uuid>,
    pub category: Option<String>,
    /// "day" | "week" | "month"
    pub bucket: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BucketPoint {
    pub bucket: String,
    pub hours: f64,
    /// Value of that bucket's tracked time, in display currency.
    pub value: f64,
    /// Hours per project id — the stacked "per project" view of the chart.
    pub by_project: std::collections::HashMap<String, f64>,
    /// Value per project id, display currency. Projects with no rate (or an unconvertible
    /// currency) contribute 0, so the stack still totals `value`.
    pub value_by_project: std::collections::HashMap<String, f64>,
}

/// Legend entry for the stacked chart: the projects present in the filtered window,
/// ordered by tracked hours descending (that order is the color assignment).
#[derive(Debug, Serialize)]
pub struct BreakdownProject {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub hours: f64,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct Breakdown {
    pub display_currency: String,
    pub total_hours: f64,
    /// Value of tracked time at each project's hourly rate, converted to display currency.
    pub total_value: f64,
    pub points: Vec<BucketPoint>,
    pub projects: Vec<BreakdownProject>,
}

/// Time breakdown grouped by day/week/month. Closed and running time both count (running
/// time uses now() as the end). Value sums each project's tracked hours × its rate, FX-
/// converted at today's rate.
pub async fn breakdown(
    pool: &PgPool,
    q: &BreakdownQuery,
    display_currency: &str,
) -> anyhow::Result<Breakdown> {
    let bucket = match q.bucket.as_deref() {
        Some("week") => "week",
        Some("month") => "month",
        _ => "day",
    };
    let since = q
        .since
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| crate::journal_fx::parse_date(s).ok());
    let until = q
        .until
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| crate::journal_fx::parse_date(s).ok());

    // Per-bucket, per-project hours. date_trunc on the entry start; running entries end at
    // now(). Grouping by project too gives both the stacked view and the value in one pass:
    // each project's rate/currency applies to its own hours.
    let sql = format!(
        "SELECT to_char(date_trunc('{bucket}', e.started_at), 'YYYY-MM-DD') AS bucket, \
                p.id, p.name, p.color, p.hourly_rate, p.rate_currency, \
                (SUM(EXTRACT(EPOCH FROM (COALESCE(e.ended_at, now()) - e.started_at))) / 3600.0)::double precision AS hours \
         FROM time_entries e JOIN time_projects p ON p.id = e.project_id \
         WHERE ($1::uuid IS NULL OR e.project_id = $1) \
           AND ($2::text IS NULL OR p.category = $2) \
           AND ($3::date IS NULL OR e.started_at >= $3) \
           AND ($4::date IS NULL OR e.started_at < ($4::date + INTERVAL '1 day')) \
         GROUP BY 1, p.id, p.name, p.color, p.hourly_rate, p.rate_currency ORDER BY 1"
    );
    type BucketRow = (String, Uuid, String, Option<String>, Option<f64>, String, Option<f64>);
    let rows = sqlx::query_as::<_, BucketRow>(sqlx::AssertSqlSafe(sql))
        .bind(q.project_id)
        .bind(q.category.as_deref().filter(|s| !s.is_empty()))
        .bind(since)
        .bind(until)
        .fetch_all(pool)
        .await
        .context("time breakdown buckets")?;

    // Hourly rate expressed in the display currency, once per project. An unconvertible
    // currency (or no rate) yields 0 — hours still count, value does not.
    let today = OffsetDateTime::now_utc().date();
    let mut fx = crate::journal_fx::FxCache::new();
    let mut rate_in_display: std::collections::HashMap<Uuid, f64> = Default::default();
    for (_, id, _, _, rate, currency, _) in &rows {
        if rate_in_display.contains_key(id) {
            continue;
        }
        let converted = match rate {
            Some(r) => fx.convert(pool, *r, currency, display_currency, today).await?.unwrap_or(0.0),
            None => 0.0,
        };
        rate_in_display.insert(*id, converted);
    }

    // Rows arrive ordered by bucket, so a bucket change opens a new point.
    let mut points: Vec<BucketPoint> = Vec::new();
    let mut legend: Vec<BreakdownProject> = Vec::new();
    let mut legend_idx: std::collections::HashMap<Uuid, usize> = Default::default();
    for (bucket, id, name, color, _, _, hours) in rows {
        let hours = hours.unwrap_or(0.0);
        let value = hours * rate_in_display.get(&id).copied().unwrap_or(0.0);
        let key = id.to_string();

        if points.last().map(|p| p.bucket.as_str()) != Some(bucket.as_str()) {
            points.push(BucketPoint {
                bucket,
                hours: 0.0,
                value: 0.0,
                by_project: Default::default(),
                value_by_project: Default::default(),
            });
        }
        let point = points.last_mut().expect("just pushed");
        point.hours += hours;
        point.value += value;
        *point.by_project.entry(key.clone()).or_insert(0.0) += hours;
        *point.value_by_project.entry(key).or_insert(0.0) += value;

        match legend_idx.get(&id) {
            Some(&i) => {
                legend[i].hours += hours;
                legend[i].value += value;
            }
            None => {
                legend_idx.insert(id, legend.len());
                legend.push(BreakdownProject { id, name, color, hours, value });
            }
        }
    }

    let total_hours: f64 = points.iter().map(|p| p.hours).sum();
    let total_value: f64 = points.iter().map(|p| p.value).sum();
    // Biggest first: the chart assigns its color ramp in this order.
    legend.sort_by(|a, b| b.hours.total_cmp(&a.hours));

    Ok(Breakdown {
        display_currency: display_currency.to_string(),
        total_hours,
        total_value,
        points,
        projects: legend,
    })
}
