//! Automator persistence: workflows, immutable versions, schedules, runs and steps.
//!
//! The graph itself is an opaque JSONB document here: its shape is owned and validated by
//! `otw-core/src/automator/`. This module only guarantees the invariants a database can
//! guarantee: revisions are append-only and numbered per workflow, a run belongs to a
//! workflow, steps belong to a run, and history is trimmed so the tables stay small.

use anyhow::Context;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Live runs kept per workflow. Test runs are trimmed on their own, smaller budget: they
/// are a debugging aid, not history.
const RUNS_KEPT: i64 = 50;
const TEST_RUNS_KEPT: i64 = 10;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub graph: Value,
    /// The editor's unsaved work, when it is not a valid graph yet. Never run.
    pub draft: Option<Value>,
    pub version_id: Option<Uuid>,
    pub mcp_token_id: Option<Uuid>,
    pub favorite: bool,
    pub enabled: bool,
    pub max_runtime_secs: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Version {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub rev: i32,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Schedule {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version_id: Option<Uuid>,
    pub kind: String,
    pub timezone: String,
    pub every_minutes: Option<i32>,
    pub at_hour: Option<i32>,
    pub at_minute: Option<i32>,
    pub weekdays: Option<i16>,
    pub day_of_month: Option<i32>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub run_at: Option<OffsetDateTime>,
    pub catch_up: bool,
    pub active: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub trigger: String,
    pub mode: String,
    pub status: String,
    pub cancel_requested: bool,
    pub input: Value,
    pub error: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    pub duration_ms: Option<i32>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RunStep {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: String,
    pub node_name: String,
    pub kind: String,
    pub seq: i32,
    pub status: String,
    pub request: Value,
    pub output: Value,
    pub error: String,
    pub bytes: i32,
    pub duration_ms: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
}

/// What a step writes when it finishes. Built by the engine, stored verbatim.
#[derive(Debug, Clone)]
pub struct StepRecord {
    pub node_id: String,
    pub node_name: String,
    pub kind: String,
    pub seq: i32,
    pub status: String,
    pub request: Value,
    pub output: Value,
    pub error: String,
    pub bytes: i32,
    pub duration_ms: i32,
}

const WF_COLS: &str = "id, name, description, graph, draft, version_id, mcp_token_id, favorite, \
                       enabled, max_runtime_secs, created_at, updated_at";
const SCHED_COLS: &str = "id, workflow_id, version_id, kind, timezone, every_minutes, at_hour, \
                          at_minute, weekdays, day_of_month, run_at, catch_up, active, \
                          next_run_at, last_run_at, created_at, updated_at";
const RUN_COLS: &str = "id, workflow_id, version_id, schedule_id, trigger, mode, status, \
                        cancel_requested, input, error, started_at, finished_at, duration_ms";
const STEP_COLS: &str = "id, run_id, node_id, node_name, kind, seq, status, request, output, \
                         error, bytes, duration_ms, started_at";

// ── Workflows ────────────────────────────────────────────────────────────────

pub async fn list_workflows(pool: &PgPool) -> anyhow::Result<Vec<Workflow>> {
    let sql = format!(
        "SELECT {WF_COLS} FROM automator_workflows ORDER BY favorite DESC, updated_at DESC"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing workflows")
}

pub async fn get_workflow(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Workflow>> {
    let sql = format!("SELECT {WF_COLS} FROM automator_workflows WHERE id = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading workflow")
}

pub async fn create_workflow(
    pool: &PgPool,
    name: &str,
    description: &str,
    graph: &Value,
) -> anyhow::Result<Workflow> {
    let sql = format!(
        "INSERT INTO automator_workflows (id, name, description, graph) \
         VALUES ($1, $2, $3, $4) RETURNING {WF_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(description)
        .bind(graph)
        .fetch_one(pool)
        .await
        .context("creating workflow")
}

/// Patch the metadata of a workflow; `None` leaves a field untouched. `token` is a double
/// option: `Some(None)` clears the permission envelope, `None` leaves it as it is.
#[allow(clippy::too_many_arguments)]
pub async fn update_workflow(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    favorite: Option<bool>,
    enabled: Option<bool>,
    max_runtime_secs: Option<i32>,
    token: Option<Option<Uuid>>,
) -> anyhow::Result<Option<Workflow>> {
    let sql = format!(
        "UPDATE automator_workflows SET \
           name = COALESCE($2, name), \
           description = COALESCE($3, description), \
           favorite = COALESCE($4, favorite), \
           enabled = COALESCE($5, enabled), \
           max_runtime_secs = COALESCE($6, max_runtime_secs), \
           mcp_token_id = CASE WHEN $7 THEN $8 ELSE mcp_token_id END, \
           updated_at = now() \
         WHERE id = $1 RETURNING {WF_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(favorite)
        .bind(enabled)
        .bind(max_runtime_secs)
        .bind(token.is_some())
        .bind(token.flatten())
        .fetch_optional(pool)
        .await
        .context("updating workflow")
}

pub async fn delete_workflow(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let done = sqlx::query("DELETE FROM automator_workflows WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting workflow")?;
    Ok(done.rows_affected() > 0)
}

// ── Versions ─────────────────────────────────────────────────────────────────

/// The note the editor's autosave writes. Consecutive revisions carrying it fold into one
/// (see `save_graph`), so a debounced save every couple of seconds does not bury the
/// revisions somebody actually meant to keep.
pub const AUTOSAVE_NOTE: &str = "auto";

/// Save a graph: writes the working copy and appends an immutable revision. Returns the
/// updated workflow and the new version row.
///
/// One exception to "append": an autosave lands on top of the previous autosave when that
/// one is still the workflow's current revision and nothing points at it. A revision a run
/// executed or a schedule is pinned to is never rewritten.
pub async fn save_graph(
    pool: &PgPool,
    id: Uuid,
    graph: &Value,
    note: &str,
) -> anyhow::Result<Option<(Workflow, Version)>> {
    let mut tx = pool.begin().await?;
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM automator_workflows WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if exists.is_none() {
        return Ok(None);
    }

    if note == AUTOSAVE_NOTE {
        let folded: Option<Version> = sqlx::query_as(
            "UPDATE automator_versions v SET graph = $2 \
             FROM automator_workflows w \
             WHERE v.id = w.version_id AND w.id = $1 AND v.note = $3 \
               AND NOT EXISTS (SELECT 1 FROM automator_runs r WHERE r.version_id = v.id) \
               AND NOT EXISTS (SELECT 1 FROM automator_schedules s WHERE s.version_id = v.id) \
             RETURNING v.id, v.workflow_id, v.rev, v.note, v.created_at",
        )
        .bind(id)
        .bind(graph)
        .bind(AUTOSAVE_NOTE)
        .fetch_optional(&mut *tx)
        .await
        .context("folding the autosave revision")?;
        if let Some(version) = folded {
            let sql = format!(
                "UPDATE automator_workflows SET graph = $2, draft = NULL, updated_at = now() \
                 WHERE id = $1 RETURNING {WF_COLS}"
            );
            let wf: Workflow = sqlx::query_as(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .bind(graph)
                .fetch_one(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(Some((wf, version)));
        }
    }

    // MAX over an empty set is one NULL row, hence Option.
    let (last,): (Option<i32>,) =
        sqlx::query_as("SELECT MAX(rev) FROM automator_versions WHERE workflow_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let rev = last.unwrap_or(0) + 1;
    let version: Version = sqlx::query_as(
        "INSERT INTO automator_versions (id, workflow_id, rev, graph, note) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id, workflow_id, rev, note, created_at",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(rev)
    .bind(graph)
    .bind(note)
    .fetch_one(&mut *tx)
    .await
    .context("appending workflow version")?;

    let sql = format!(
        "UPDATE automator_workflows SET graph = $2, draft = NULL, version_id = $3, \
         updated_at = now() WHERE id = $1 RETURNING {WF_COLS}"
    );
    let wf: Workflow = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(graph)
        .bind(version.id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some((wf, version)))
}

/// Park the editor's work when it is not a runnable graph yet. Touches nothing else: the
/// graph the schedules run is the last valid one, and stays that way.
pub async fn save_draft(pool: &PgPool, id: Uuid, draft: &Value) -> anyhow::Result<Option<Workflow>> {
    let sql = format!(
        "UPDATE automator_workflows SET draft = $2 WHERE id = $1 RETURNING {WF_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(draft)
        .fetch_optional(pool)
        .await
        .context("saving the workflow draft")
}

pub async fn list_versions(pool: &PgPool, workflow_id: Uuid) -> anyhow::Result<Vec<Version>> {
    sqlx::query_as(
        "SELECT id, workflow_id, rev, note, created_at FROM automator_versions \
         WHERE workflow_id = $1 ORDER BY rev DESC LIMIT 100",
    )
    .bind(workflow_id)
    .fetch_all(pool)
    .await
    .context("listing workflow versions")
}

/// The stored graph of one revision (used by a pinned schedule and by rollback).
pub async fn version_graph(pool: &PgPool, version_id: Uuid) -> anyhow::Result<Option<Value>> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT graph FROM automator_versions WHERE id = $1")
            .bind(version_id)
            .fetch_optional(pool)
            .await
            .context("loading version graph")?;
    Ok(row.map(|r| r.0))
}

/// Point every schedule of a workflow at one revision (`None` = follow the latest).
pub async fn repoint_schedules(
    pool: &PgPool,
    workflow_id: Uuid,
    version_id: Option<Uuid>,
) -> anyhow::Result<u64> {
    let done = sqlx::query(
        "UPDATE automator_schedules SET version_id = $2, updated_at = now() WHERE workflow_id = $1",
    )
    .bind(workflow_id)
    .bind(version_id)
    .execute(pool)
    .await
    .context("repointing schedules")?;
    Ok(done.rows_affected())
}

// ── Schedules ────────────────────────────────────────────────────────────────

/// Every field of a recurrence rule. The engine computes `next_run_at` before writing.
#[derive(Debug, Clone)]
pub struct ScheduleInput {
    pub workflow_id: Uuid,
    pub version_id: Option<Uuid>,
    pub kind: String,
    pub timezone: String,
    pub every_minutes: Option<i32>,
    pub at_hour: Option<i32>,
    pub at_minute: Option<i32>,
    pub weekdays: Option<i16>,
    pub day_of_month: Option<i32>,
    pub run_at: Option<OffsetDateTime>,
    pub catch_up: bool,
    pub active: bool,
    pub next_run_at: Option<OffsetDateTime>,
}

pub async fn list_schedules(
    pool: &PgPool,
    workflow_id: Option<Uuid>,
) -> anyhow::Result<Vec<Schedule>> {
    let sql = format!(
        "SELECT {SCHED_COLS} FROM automator_schedules \
         WHERE ($1::uuid IS NULL OR workflow_id = $1) \
         ORDER BY active DESC, next_run_at NULLS LAST"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(workflow_id)
        .fetch_all(pool)
        .await
        .context("listing schedules")
}

pub async fn get_schedule(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Schedule>> {
    let sql = format!("SELECT {SCHED_COLS} FROM automator_schedules WHERE id = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading schedule")
}

pub async fn create_schedule(pool: &PgPool, input: &ScheduleInput) -> anyhow::Result<Schedule> {
    let sql = format!(
        "INSERT INTO automator_schedules (id, workflow_id, version_id, kind, timezone, \
             every_minutes, at_hour, at_minute, weekdays, day_of_month, run_at, catch_up, \
             active, next_run_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
         RETURNING {SCHED_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(input.workflow_id)
        .bind(input.version_id)
        .bind(&input.kind)
        .bind(&input.timezone)
        .bind(input.every_minutes)
        .bind(input.at_hour)
        .bind(input.at_minute)
        .bind(input.weekdays)
        .bind(input.day_of_month)
        .bind(input.run_at)
        .bind(input.catch_up)
        .bind(input.active)
        .bind(input.next_run_at)
        .fetch_one(pool)
        .await
        .context("creating schedule")
}

/// Replace a schedule's rule wholesale (the API reads the row, applies the patch, and
/// recomputes `next_run_at`, so there is one place where a rule becomes an instant).
pub async fn update_schedule(
    pool: &PgPool,
    id: Uuid,
    input: &ScheduleInput,
) -> anyhow::Result<Option<Schedule>> {
    let sql = format!(
        "UPDATE automator_schedules SET version_id = $2, kind = $3, timezone = $4, \
             every_minutes = $5, at_hour = $6, at_minute = $7, weekdays = $8, \
             day_of_month = $9, run_at = $10, catch_up = $11, active = $12, \
             next_run_at = $13, updated_at = now() \
         WHERE id = $1 RETURNING {SCHED_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.version_id)
        .bind(&input.kind)
        .bind(&input.timezone)
        .bind(input.every_minutes)
        .bind(input.at_hour)
        .bind(input.at_minute)
        .bind(input.weekdays)
        .bind(input.day_of_month)
        .bind(input.run_at)
        .bind(input.catch_up)
        .bind(input.active)
        .bind(input.next_run_at)
        .fetch_optional(pool)
        .await
        .context("updating schedule")
}

pub async fn delete_schedule(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let done = sqlx::query("DELETE FROM automator_schedules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting schedule")?;
    Ok(done.rows_affected() > 0)
}

/// Claim the schedules that are due and whose workflow is idle.
///
/// Two rules live in this query, both deliberate: a workflow that is still running never
/// starts a second run (the occurrence is dropped, not queued), and a disabled workflow is
/// never triggered by a schedule. `FOR UPDATE ... SKIP LOCKED` keeps it safe if the tick
/// ever runs twice.
pub async fn claim_due_schedules(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Schedule>> {
    let sql = format!(
        "SELECT {} FROM automator_schedules s \
           JOIN automator_workflows w ON w.id = s.workflow_id \
          WHERE s.active AND w.enabled AND s.next_run_at IS NOT NULL AND s.next_run_at <= now() \
            AND NOT EXISTS (SELECT 1 FROM automator_runs r \
                             WHERE r.workflow_id = s.workflow_id \
                               AND r.status IN ('queued', 'running')) \
          ORDER BY s.next_run_at LIMIT $1 \
            FOR UPDATE OF s SKIP LOCKED",
        SCHED_COLS
            .split(", ")
            .map(|c| format!("s.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("claiming due schedules")
}

/// Advance a schedule after a run was started for it.
pub async fn mark_schedule_ran(
    pool: &PgPool,
    id: Uuid,
    next_run_at: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE automator_schedules SET last_run_at = now(), next_run_at = $2, \
             active = CASE WHEN kind = 'once' THEN FALSE ELSE active END, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(next_run_at)
    .execute(pool)
    .await
    .context("advancing schedule")?;
    Ok(())
}

/// Re-plan a schedule without running it (used when a rule is re-resolved at boot).
pub async fn set_next_run(
    pool: &PgPool,
    id: Uuid,
    next_run_at: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE automator_schedules SET next_run_at = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(next_run_at)
        .execute(pool)
        .await
        .context("setting next run")?;
    Ok(())
}

/// Active schedules whose next occurrence is already in the past: the boot catch-up set.
pub async fn overdue_schedules(pool: &PgPool) -> anyhow::Result<Vec<Schedule>> {
    let sql = format!(
        "SELECT {SCHED_COLS} FROM automator_schedules \
          WHERE active AND next_run_at IS NOT NULL AND next_run_at <= now() \
          ORDER BY next_run_at"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing overdue schedules")
}

// ── Runs ─────────────────────────────────────────────────────────────────────

/// Open a run, refusing if the workflow already has one queued or running. Returns `None`
/// on that refusal, which the API turns into a 409.
pub async fn start_run(
    pool: &PgPool,
    workflow_id: Uuid,
    version_id: Option<Uuid>,
    schedule_id: Option<Uuid>,
    trigger: &str,
    mode: &str,
    input: &Value,
) -> anyhow::Result<Option<Run>> {
    let sql = format!(
        "INSERT INTO automator_runs (id, workflow_id, version_id, schedule_id, trigger, mode, \
             status, input) \
         SELECT $1, $2, $3, $4, $5, $6, 'running', $7 \
          WHERE NOT EXISTS (SELECT 1 FROM automator_runs r \
                             WHERE r.workflow_id = $2 AND r.status IN ('queued', 'running')) \
         RETURNING {RUN_COLS}"
    );
    let run: Option<Run> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(workflow_id)
        .bind(version_id)
        .bind(schedule_id)
        .bind(trigger)
        .bind(mode)
        .bind(input)
        .fetch_optional(pool)
        .await
        .context("starting run")?;
    if run.is_some() {
        trim_runs(pool, workflow_id).await?;
    }
    Ok(run)
}

/// Close a run with its final status.
pub async fn finish_run(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    error: &str,
    duration_ms: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE automator_runs SET status = $2, error = $3, duration_ms = $4, \
             finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .bind(duration_ms)
    .execute(pool)
    .await
    .context("finishing run")?;
    Ok(())
}

pub async fn request_cancel(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let done = sqlx::query(
        "UPDATE automator_runs SET cancel_requested = TRUE \
         WHERE id = $1 AND status IN ('queued', 'running')",
    )
    .bind(id)
    .execute(pool)
    .await
    .context("requesting run cancel")?;
    Ok(done.rows_affected() > 0)
}

pub async fn cancel_requested(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let row: Option<(bool,)> =
        sqlx::query_as("SELECT cancel_requested FROM automator_runs WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("reading cancel flag")?;
    Ok(row.map(|r| r.0).unwrap_or(false))
}

/// Runs that outlived their workflow's `max_runtime_secs` (the watchdog's input).
pub async fn stale_runs(pool: &PgPool) -> anyhow::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM automator_runs r JOIN automator_workflows w ON w.id = r.workflow_id \
          WHERE r.status IN ('queued', 'running') \
            AND r.started_at < now() - make_interval(secs => w.max_runtime_secs)",
    )
    .fetch_all(pool)
    .await
    .context("listing stale runs")?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn list_runs(
    pool: &PgPool,
    workflow_id: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<Run>> {
    let sql = format!(
        "SELECT {RUN_COLS} FROM automator_runs \
          WHERE ($1::uuid IS NULL OR workflow_id = $1) \
            AND ($2::text IS NULL OR status = $2) \
          ORDER BY started_at DESC LIMIT $3"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(workflow_id)
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("listing runs")
}

pub async fn get_run(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Run>> {
    let sql = format!("SELECT {RUN_COLS} FROM automator_runs WHERE id = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading run")
}

pub async fn run_steps(pool: &PgPool, run_id: Uuid) -> anyhow::Result<Vec<RunStep>> {
    let sql = format!("SELECT {STEP_COLS} FROM automator_run_steps WHERE run_id = $1 ORDER BY seq");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(run_id)
        .fetch_all(pool)
        .await
        .context("listing run steps")
}

pub async fn add_step(pool: &PgPool, run_id: Uuid, step: &StepRecord) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO automator_run_steps (id, run_id, node_id, node_name, kind, seq, status, \
             request, output, error, bytes, duration_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(Uuid::new_v4())
    .bind(run_id)
    .bind(&step.node_id)
    .bind(&step.node_name)
    .bind(&step.kind)
    .bind(step.seq)
    .bind(&step.status)
    .bind(&step.request)
    .bind(&step.output)
    .bind(&step.error)
    .bind(step.bytes)
    .bind(step.duration_ms)
    .execute(pool)
    .await
    .context("recording run step")?;
    Ok(())
}

/// Keep the newest runs per workflow; live and test histories are trimmed separately so a
/// burst of test runs cannot evict the scheduled history.
async fn trim_runs(pool: &PgPool, workflow_id: Uuid) -> anyhow::Result<()> {
    for (mode, keep) in [("live", RUNS_KEPT), ("test", TEST_RUNS_KEPT)] {
        sqlx::query(
            "DELETE FROM automator_runs WHERE id IN ( \
                 SELECT id FROM automator_runs \
                  WHERE workflow_id = $1 AND mode = $2 \
                  ORDER BY started_at DESC OFFSET $3)",
        )
        .bind(workflow_id)
        .bind(mode)
        .bind(keep)
        .execute(pool)
        .await
        .context("trimming run history")?;
    }
    Ok(())
}

// ── Blobs ────────────────────────────────────────────────────────────────────

pub async fn put_blob(
    pool: &PgPool,
    run_id: Uuid,
    node_id: &str,
    content_type: &str,
    data: &[u8],
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO automator_blobs (id, run_id, node_id, content_type, bytes, data) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(run_id)
    .bind(node_id)
    .bind(content_type)
    .bind(data.len() as i32)
    .bind(data)
    .execute(pool)
    .await
    .context("storing run blob")?;
    Ok(id)
}

/// A stored payload: `(content_type, bytes)`.
pub async fn get_blob(
    pool: &PgPool,
    run_id: Uuid,
    id: Uuid,
) -> anyhow::Result<Option<(String, Vec<u8>)>> {
    let row: Option<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT content_type, data FROM automator_blobs WHERE id = $1 AND run_id = $2",
    )
    .bind(id)
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .context("loading run blob")?;
    Ok(row)
}

/// Summary shown next to a workflow in the list: last run status and when.
pub async fn last_runs(pool: &PgPool) -> anyhow::Result<Value> {
    let rows: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT DISTINCT ON (workflow_id) workflow_id, status, started_at \
           FROM automator_runs WHERE mode = 'live' \
          ORDER BY workflow_id, started_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing last runs")?;
    let mut out = serde_json::Map::new();
    for (wf, status, at) in rows {
        out.insert(
            wf.to_string(),
            json!({ "status": status, "at": at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }),
        );
    }
    Ok(Value::Object(out))
}
