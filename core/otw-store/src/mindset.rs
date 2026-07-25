//! Storage for the Mindset module.
//!
//! Two check-ins per trading day — a pre-mortem before the session and a post-mortem after —
//! each built from customizable prompts (scale 1–5, single choice, multi tags, free text).
//! Answers are one JSONB map per (date, phase) keyed by prompt id, so prompts can evolve
//! without migrating old entries. Default prompts are seeded on first read; users edit,
//! disable, reorder or replace them. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Prompt {
    pub id: Uuid,
    pub phase: String,
    pub kind: String,
    pub label: String,
    pub config: Value,
    pub position: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    #[serde(with = "date_fmt")]
    pub entry_date: Date,
    pub phase: String,
    pub answers: Value,
}

mod date_fmt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

const PROMPT_COLS: &str = "id, phase, kind, label, config, position, active";

// ── Default prompts ──────────────────────────────────────────────────────────

/// (phase, kind, label, config) for the seeded starter set.
fn default_prompts() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    let moods = json!({ "options": ["😞", "😕", "😐", "🙂", "😄"] });
    let states = json!({
        "options": ["calm", "confident", "disciplined", "anxious", "impatient", "FOMO", "tired", "distracted"]
    });
    vec![
        ("pre", "scale", "Sleep quality", json!({ "low": "Poor", "high": "Great" })),
        ("pre", "scale", "Energy", json!({ "low": "Drained", "high": "Sharp" })),
        ("pre", "scale", "Stress", json!({ "low": "Calm", "high": "Wired" })),
        ("pre", "choice", "Mood", moods.clone()),
        ("pre", "tags", "State of mind", states.clone()),
        (
            "pre",
            "text",
            "Pre-mortem: it's the close and today went badly — what happened?",
            json!({}),
        ),
        ("pre", "text", "What will you do to prevent that?", json!({})),
        ("post", "scale", "Discipline (followed the plan)", json!({ "low": "Not at all", "high": "Fully" })),
        ("post", "scale", "Emotional control", json!({ "low": "Reactive", "high": "Composed" })),
        ("post", "scale", "Execution quality", json!({ "low": "Sloppy", "high": "Clean" })),
        ("post", "choice", "How the session felt", moods),
        ("post", "tags", "Emotions during the session", states),
        ("post", "text", "What went well?", json!({})),
        ("post", "text", "What will you improve tomorrow?", json!({})),
        // ── Trader-wisdom prompts, paraphrased and attributed by name only. ───────
        // Ideas/principles aren't copyrightable; we credit the source without quoting.
        (
            "pre",
            "text",
            "Kovner: what's my invalidation, and can I lose here without it hurting? (Bruce Kovner)",
            json!({}),
        ),
        (
            "pre",
            "text",
            "Pre-mortem: assume this trade is a loss — where was the mistake? (Bruce Kovner)",
            json!({}),
        ),
        (
            "pre",
            "choice",
            "Am I trading in harmony with my own temperament today? (Ed Seykota)",
            json!({ "options": ["Yes — my style", "Forcing it", "Chasing", "Unsure"] }),
        ),
        (
            "pre",
            "text",
            "Douglas: I accept the risk on this trade — thinking in probabilities, not certainties. (Mark Douglas)",
            json!({}),
        ),
        (
            "pre",
            "scale",
            "Am I willing to sit and wait for my setup? (Jesse Livermore)",
            json!({ "low": "Itchy", "high": "Patient" }),
        ),
        (
            "post",
            "choice",
            "Did I cut the loser fast, or hope? (Ed Seykota)",
            json!({ "options": ["Cut on plan", "Hesitated", "Averaged down", "No loser today"] }),
        ),
        (
            "post",
            "scale",
            "Did I let winners run, or clip them early? (Paul Tudor Jones)",
            json!({ "low": "Clipped", "high": "Let it run" }),
        ),
        (
            "post",
            "choice",
            "Was any trade emotional (revenge, FOMO, boredom)? (Mark Douglas)",
            json!({ "options": ["All process", "One slip", "Several", "Tilted"] }),
        ),
        (
            "post",
            "text",
            "Pain + reflection = progress: what mistake, and what principle do I take from it? (Ray Dalio)",
            json!({}),
        ),
        (
            "post",
            "text",
            "Graded the process, not the PnL — what would I repeat regardless of outcome? (Mark Minervini)",
            json!({}),
        ),
    ]
}

/// Settings flag marking that the starter set was installed once. Seeding is one-shot so a
/// user can delete every prompt and build their own from scratch without defaults respawning.
const SEEDED_KEY: &str = "mindset_prompts_seeded";

/// Install the starter prompts on first use only (never after the user has touched the set).
pub async fn seed_if_first_run(pool: &PgPool) -> anyhow::Result<()> {
    if crate::settings::get(pool, SEEDED_KEY).await?.is_some() {
        return Ok(());
    }
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mindset_prompts")
        .fetch_one(pool)
        .await
        .context("counting prompts")?;
    if count == 0 {
        for (i, (phase, kind, label, config)) in default_prompts().into_iter().enumerate() {
            add_prompt(pool, phase, kind, label, &config, i as f64).await?;
        }
    }
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

/// Delete every prompt (start from scratch). Past entries keep their answers; values keyed
/// by deleted prompt ids simply stop rendering.
pub async fn clear_prompts(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_prompts")
        .execute(pool)
        .await
        .context("clearing prompts")?;
    // Ensure the seed flag exists so the defaults don't respawn on the next read.
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

// ── Prompts CRUD ─────────────────────────────────────────────────────────────

pub async fn list_prompts(pool: &PgPool, only_active: bool) -> anyhow::Result<Vec<Prompt>> {
    let sql = format!(
        "SELECT {PROMPT_COLS} FROM mindset_prompts {} ORDER BY position, created_at",
        if only_active { "WHERE active" } else { "" }
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing prompts")?)
}

pub async fn add_prompt(
    pool: &PgPool,
    phase: &str,
    kind: &str,
    label: &str,
    config: &Value,
    position: f64,
) -> anyhow::Result<Prompt> {
    let sql = format!(
        "INSERT INTO mindset_prompts (id, phase, kind, label, config, position) \
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING {PROMPT_COLS}"
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(phase)
        .bind(kind)
        .bind(label)
        .bind(config)
        .bind(position)
        .fetch_one(pool)
        .await
        .context("adding prompt")?)
}

/// Append position for a phase (new prompts land at the end of their check-in).
pub async fn next_position(pool: &PgPool, phase: &str) -> anyhow::Result<f64> {
    let (max,): (Option<f64>,) =
        sqlx::query_as("SELECT MAX(position) FROM mindset_prompts WHERE phase = $1")
            .bind(phase)
            .fetch_one(pool)
            .await
            .context("reading max prompt position")?;
    Ok(max.unwrap_or(0.0) + 1.0)
}

pub async fn update_prompt(
    pool: &PgPool,
    id: Uuid,
    label: Option<&str>,
    config: Option<&Value>,
    position: Option<f64>,
    active: Option<bool>,
) -> anyhow::Result<Option<Prompt>> {
    let sql = format!(
        "UPDATE mindset_prompts SET \
         label = COALESCE($2, label), \
         config = COALESCE($3, config), \
         position = COALESCE($4, position), \
         active = COALESCE($5, active) \
         WHERE id = $1 RETURNING {PROMPT_COLS}"
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(label)
        .bind(config)
        .bind(position)
        .bind(active)
        .fetch_optional(pool)
        .await
        .context("updating prompt")?)
}

pub async fn delete_prompt(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_prompts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting prompt")?;
    Ok(())
}

/// Wipe custom prompts and reinstall the starter set (entries keep their answers; values
/// keyed by deleted prompt ids simply stop rendering).
pub async fn reset_prompts(pool: &PgPool) -> anyhow::Result<()> {
    clear_prompts(pool).await?;
    for (i, (phase, kind, label, config)) in default_prompts().into_iter().enumerate() {
        add_prompt(pool, phase, kind, label, &config, i as f64).await?;
    }
    Ok(())
}

// ── Entries ──────────────────────────────────────────────────────────────────

pub async fn entries_for_date(pool: &PgPool, date: Date) -> anyhow::Result<Vec<Entry>> {
    Ok(sqlx::query_as::<_, Entry>(
        "SELECT id, entry_date, phase, answers FROM mindset_entries WHERE entry_date = $1",
    )
    .bind(date)
    .fetch_all(pool)
    .await
    .context("listing day entries")?)
}

/// Upsert the (date, phase) check-in with a full answers map.
pub async fn save_entry(
    pool: &PgPool,
    date: Date,
    phase: &str,
    answers: &Value,
) -> anyhow::Result<Entry> {
    Ok(sqlx::query_as::<_, Entry>(
        "INSERT INTO mindset_entries (id, entry_date, phase, answers) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (entry_date, phase) DO UPDATE SET answers = EXCLUDED.answers, updated_at = now() \
         RETURNING id, entry_date, phase, answers",
    )
    .bind(Uuid::new_v4())
    .bind(date)
    .bind(phase)
    .bind(answers)
    .fetch_one(pool)
    .await
    .context("saving entry")?)
}

pub async fn delete_entry(pool: &PgPool, date: Date, phase: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_entries WHERE entry_date = $1 AND phase = $2")
        .bind(date)
        .bind(phase)
        .execute(pool)
        .await
        .context("deleting entry")?;
    Ok(())
}

/// Most recent entries (newest first), capped, for the history list and trend charts.
pub async fn recent_entries(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Entry>> {
    Ok(sqlx::query_as::<_, Entry>(
        "SELECT id, entry_date, phase, answers FROM mindset_entries \
         ORDER BY entry_date DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing recent entries")?)
}
