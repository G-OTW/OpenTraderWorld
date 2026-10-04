//! Storage for the Mindset module.
//!
//! A **template** is a named, categorised check-in: a set of prompts (scale 1–5, single
//! choice, multi tags, free text) bound to a phase of the trading day (`pre` before the
//! session, `post` after). Answers are one JSONB map per (date, template) keyed by prompt
//! id, so prompts can evolve without migrating old entries.
//!
//! Consistency is a per-day verdict the user sets by hand (`full` / `action`) rather than
//! something inferred from filled fields — the same reasoning as the routines board: only
//! the trader knows whether a day counted. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::Date;
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
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub notes: String,
    pub category_id: Option<Uuid>,
    pub phase: String,
    pub position: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Prompt {
    pub id: Uuid,
    pub template_id: Uuid,
    pub phase: String,
    pub kind: String,
    pub label: String,
    pub hint: String,
    pub config: Value,
    pub position: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    #[serde(with = "date_fmt")]
    pub entry_date: Date,
    pub template_id: Uuid,
    pub phase: String,
    pub answers: Value,
}

/// One day's verdict: `full` (checked in and followed through) or `action` (traded without).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DayMark {
    #[serde(with = "date_fmt")]
    pub day: Date,
    pub mark: String,
}

pub const DAY_MARKS: &[&str] = &["full", "action"];

mod date_fmt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

const PROMPT_COLS: &str =
    "id, template_id, phase, kind, label, hint, config, position, active";
const TEMPLATE_COLS: &str =
    "id, name, description, notes, category_id, phase, position, active";
// ── Default templates ────────────────────────────────────────────────────────

/// A prompt in the starter set: (kind, label, hint, config).
type SeedPrompt = (&'static str, &'static str, &'static str, Value);

/// The starter categories: (name, colour).
///
/// Named for *what kind of check-in* they hold, never for when it runs — timing is the
/// `phase` field, and a category repeating a phase label would show the same words twice
/// in the editor.
pub const DEFAULT_CATEGORIES: &[(&str, &str)] = &[
    ("Daily", "var(--chart-1)"),
    ("Trader wisdom", "var(--chart-3)"),
    ("Weekly review", "var(--chart-5)"),
];

/// The seeded templates: (category, name, phase, description, prompts).
///
/// The trader-wisdom sets are paraphrased from public principles and credited by name
/// only — ideas aren't copyrightable and we quote nothing. They install once as ordinary,
/// fully-editable templates the user can rename, tweak or delete like their own.
fn default_templates() -> Vec<(&'static str, &'static str, &'static str, &'static str, Vec<SeedPrompt>)>
{
    let moods = json!({ "options": ["😞", "😕", "😐", "🙂", "😄"] });
    let states = json!({
        "options": ["calm", "confident", "disciplined", "anxious", "impatient", "FOMO", "tired", "distracted"]
    });
    vec![
        (
            "Daily",
            "Pre-mortem",
            "pre",
            "Set the state and name the risks before the first trade.",
            vec![
                ("scale", "Sleep quality", "", json!({ "low": "Poor", "high": "Great" })),
                ("scale", "Energy", "", json!({ "low": "Drained", "high": "Sharp" })),
                ("scale", "Stress", "", json!({ "low": "Calm", "high": "Wired" })),
                ("choice", "Mood", "", moods.clone()),
                ("tags", "State of mind", "", states.clone()),
                (
                    "text",
                    "It's the close and today went badly — what happened?",
                    "Write the failure before it happens; it stops being a surprise.",
                    json!({}),
                ),
                ("text", "What will you do to prevent that?", "", json!({})),
            ],
        ),
        (
            "Daily",
            "Post-mortem",
            "post",
            "Grade the process, not the PnL.",
            vec![
                ("scale", "Discipline (followed the plan)", "", json!({ "low": "Not at all", "high": "Fully" })),
                ("scale", "Emotional control", "", json!({ "low": "Reactive", "high": "Composed" })),
                ("scale", "Execution quality", "", json!({ "low": "Sloppy", "high": "Clean" })),
                ("choice", "How the session felt", "", moods),
                ("tags", "Emotions during the session", "", states),
                ("text", "What went well?", "", json!({})),
                ("text", "What will you improve tomorrow?", "", json!({})),
            ],
        ),
        (
            "Trader wisdom",
            "Risk & temperament",
            "pre",
            "Five questions the greats ask themselves before risking anything.",
            vec![
                (
                    "text",
                    "What's my invalidation, and can I lose here without it hurting? (Bruce Kovner)",
                    "",
                    json!({}),
                ),
                (
                    "text",
                    "Assume this trade is a loss — where was the mistake? (Bruce Kovner)",
                    "",
                    json!({}),
                ),
                (
                    "choice",
                    "Am I trading in harmony with my own temperament today? (Ed Seykota)",
                    "",
                    json!({ "options": ["Yes — my style", "Forcing it", "Chasing", "Unsure"] }),
                ),
                (
                    "text",
                    "I accept the risk on this trade — probabilities, not certainties. (Mark Douglas)",
                    "",
                    json!({}),
                ),
                (
                    "scale",
                    "Am I willing to sit and wait for my setup? (Jesse Livermore)",
                    "",
                    json!({ "low": "Itchy", "high": "Patient" }),
                ),
            ],
        ),
        (
            "Trader wisdom",
            "Process review",
            "post",
            "What the tape can't tell you: how you behaved.",
            vec![
                (
                    "choice",
                    "Did I cut the loser fast, or hope? (Ed Seykota)",
                    "",
                    json!({ "options": ["Cut on plan", "Hesitated", "Averaged down", "No loser today"] }),
                ),
                (
                    "scale",
                    "Did I let winners run, or clip them early? (Paul Tudor Jones)",
                    "",
                    json!({ "low": "Clipped", "high": "Let it run" }),
                ),
                (
                    "choice",
                    "Was any trade emotional (revenge, FOMO, boredom)? (Mark Douglas)",
                    "",
                    json!({ "options": ["All process", "One slip", "Several", "Tilted"] }),
                ),
                (
                    "text",
                    "Pain + reflection = progress: what mistake, what principle? (Ray Dalio)",
                    "",
                    json!({}),
                ),
                (
                    "text",
                    "What would I repeat regardless of outcome? (Mark Minervini)",
                    "",
                    json!({}),
                ),
            ],
        ),
    ]
}

/// Settings flag marking that the starter set was installed once. Seeding is one-shot so a
/// user can delete every template and build their own from scratch without defaults
/// respawning on the next read.
const SEEDED_KEY: &str = "mindset_prompts_seeded";
/// Categories arrived with 0080; an install seeded before it has the flag above set but
/// only the two backfilled categories, so they get their own one-shot guard.
const CATS_SEEDED_KEY: &str = "mindset_categories_seeded";

/// Install the starter categories and templates on first use only.
pub async fn seed_if_first_run(pool: &PgPool) -> anyhow::Result<()> {
    ensure_categories(pool).await?;
    if crate::settings::get(pool, SEEDED_KEY).await?.is_some() {
        return Ok(());
    }
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mindset_templates")
        .fetch_one(pool)
        .await
        .context("counting templates")?;
    if count == 0 {
        install_defaults(pool).await?;
    }
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

/// Create the starter categories once, skipping names that already exist (0080 backfills
/// two of them, and they must be matched rather than duplicated).
async fn ensure_categories(pool: &PgPool) -> anyhow::Result<()> {
    if crate::settings::get(pool, CATS_SEEDED_KEY).await?.is_some() {
        return Ok(());
    }
    let existing: Vec<String> = sqlx::query_scalar("SELECT name FROM mindset_categories")
        .fetch_all(pool)
        .await
        .context("listing category names")?;
    for (i, (name, color)) in DEFAULT_CATEGORIES.iter().enumerate() {
        if existing.iter().any(|e| e == name) {
            continue;
        }
        sqlx::query(
            "INSERT INTO mindset_categories (id, name, color, position) VALUES ($1,$2,$3,$4)",
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

async fn install_defaults(pool: &PgPool) -> anyhow::Result<()> {
    let cats = list_categories(pool).await?;
    for (i, (cat, name, phase, description, prompts)) in default_templates().into_iter().enumerate()
    {
        let category_id = cats.iter().find(|c| c.name == cat).map(|c| c.id);
        let tpl = create_template(
            pool,
            NewTemplate {
                name: name.to_owned(),
                description: description.to_owned(),
                notes: String::new(),
                category_id,
                phase: phase.to_owned(),
            },
        )
        .await?;
        sqlx::query("UPDATE mindset_templates SET position = $2 WHERE id = $1")
            .bind(tpl.id)
            .bind(i as f64)
            .execute(pool)
            .await
            .context("ordering seeded template")?;
        for (j, (kind, label, hint, config)) in prompts.into_iter().enumerate() {
            add_prompt(
                pool,
                tpl.id,
                phase,
                kind,
                label,
                hint,
                &config,
                j as f64,
            )
            .await?;
        }
    }
    Ok(())
}

/// Delete every template (and its prompts, by cascade) so the user can start from scratch.
/// Past entries keep their answers; values keyed by deleted prompt ids stop rendering.
pub async fn clear_templates(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_templates")
        .execute(pool)
        .await
        .context("clearing templates")?;
    // Ensure the seed flag exists so the defaults don't respawn on the next read.
    crate::settings::set(pool, SEEDED_KEY, "1").await?;
    Ok(())
}

/// Wipe everything and reinstall the starter set.
pub async fn reset_templates(pool: &PgPool) -> anyhow::Result<()> {
    clear_templates(pool).await?;
    install_defaults(pool).await?;
    Ok(())
}

// ── Categories ───────────────────────────────────────────────────────────────

pub async fn list_categories(pool: &PgPool) -> anyhow::Result<Vec<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "SELECT id, name, color, position FROM mindset_categories ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing categories")?)
}

pub async fn create_category(pool: &PgPool, name: &str, color: &str) -> anyhow::Result<Category> {
    Ok(sqlx::query_as::<_, Category>(
        "INSERT INTO mindset_categories (id, name, color, position) \
         VALUES ($1,$2,$3,(SELECT COALESCE(MAX(position),0)+1 FROM mindset_categories)) \
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
        "UPDATE mindset_categories SET \
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

/// Delete a category; its templates survive as uncategorised (`ON DELETE SET NULL`).
pub async fn delete_category(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_categories WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting category")?;
    Ok(())
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewTemplate {
    pub name: String,
    pub description: String,
    pub notes: String,
    pub category_id: Option<Uuid>,
    pub phase: String,
}

/// Fields a PATCH may change; `Some(None)` on `category_id` clears it.
#[derive(Debug, Clone, Default)]
pub struct TemplatePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub category_id: Option<Option<Uuid>>,
    pub phase: Option<String>,
    pub position: Option<f64>,
    pub active: Option<bool>,
}

pub async fn list_templates(pool: &PgPool, only_active: bool) -> anyhow::Result<Vec<Template>> {
    let sql = format!(
        "SELECT {TEMPLATE_COLS} FROM mindset_templates {} ORDER BY position, created_at",
        if only_active { "WHERE active" } else { "" }
    );
    Ok(sqlx::query_as::<_, Template>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing templates")?)
}

pub async fn get_template(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Template>> {
    let sql = format!("SELECT {TEMPLATE_COLS} FROM mindset_templates WHERE id = $1");
    Ok(sqlx::query_as::<_, Template>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching template")?)
}

pub async fn create_template(pool: &PgPool, t: NewTemplate) -> anyhow::Result<Template> {
    let sql = format!(
        "INSERT INTO mindset_templates (id, name, description, notes, category_id, phase, position) \
         VALUES ($1,$2,$3,$4,$5,$6,(SELECT COALESCE(MAX(position),0)+1 FROM mindset_templates)) \
         RETURNING {TEMPLATE_COLS}"
    );
    Ok(sqlx::query_as::<_, Template>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(&t.name)
        .bind(&t.description)
        .bind(&t.notes)
        .bind(t.category_id)
        .bind(&t.phase)
        .fetch_one(pool)
        .await
        .context("creating template")?)
}

pub async fn update_template(
    pool: &PgPool,
    id: Uuid,
    p: TemplatePatch,
) -> anyhow::Result<Option<Template>> {
    let sql = format!(
        "UPDATE mindset_templates SET \
         name = COALESCE($2, name), \
         description = COALESCE($3, description), \
         notes = COALESCE($4, notes), \
         category_id = CASE WHEN $5 THEN $6 ELSE category_id END, \
         phase = COALESCE($7, phase), \
         position = COALESCE($8, position), \
         active = COALESCE($9, active), \
         updated_at = now() \
         WHERE id = $1 RETURNING {TEMPLATE_COLS}"
    );
    let updated = sqlx::query_as::<_, Template>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(p.name.as_deref())
        .bind(p.description.as_deref())
        .bind(p.notes.as_deref())
        .bind(p.category_id.is_some())
        .bind(p.category_id.flatten())
        .bind(p.phase.as_deref())
        .bind(p.position)
        .bind(p.active)
        .fetch_optional(pool)
        .await
        .context("updating template")?;
    // Prompts carry a denormalised `phase` so the day view can bucket them without a join;
    // moving a template between phases has to carry its prompts along.
    if let (Some(t), Some(_)) = (&updated, &p.phase) {
        sqlx::query("UPDATE mindset_prompts SET phase = $2 WHERE template_id = $1")
            .bind(t.id)
            .bind(&t.phase)
            .execute(pool)
            .await
            .context("realigning prompt phases")?;
    }
    Ok(updated)
}

pub async fn delete_template(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_templates WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting template")?;
    Ok(())
}

/// Copy a template with its prompts (answers excluded) under a new name.
pub async fn duplicate_template(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<Template> {
    let src = get_template(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("template not found"))?;
    let prompts = list_prompts_for(pool, id).await?;
    let copy = create_template(
        pool,
        NewTemplate {
            name: name.to_owned(),
            description: src.description,
            notes: src.notes,
            category_id: src.category_id,
            phase: src.phase.clone(),
        },
    )
    .await?;
    for (i, p) in prompts.into_iter().enumerate() {
        add_prompt(
            pool,
            copy.id,
            &src.phase,
            &p.kind,
            &p.label,
            &p.hint,
            &p.config,
            i as f64,
        )
        .await?;
    }
    Ok(copy)
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

/// Active prompts of the active templates — what the day view actually renders.
pub async fn day_prompts(pool: &PgPool) -> anyhow::Result<Vec<Prompt>> {
    let sql = format!(
        "SELECT p.id, p.template_id, p.phase, p.kind, p.label, p.hint, p.config, p.position, \
                p.active \
         FROM mindset_prompts p JOIN mindset_templates t ON t.id = p.template_id \
         WHERE p.active AND t.active ORDER BY p.position, p.created_at"
    );
    let _ = PROMPT_COLS; // columns are qualified above; the const documents the shape
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing day prompts")?)
}

pub async fn list_prompts_for(pool: &PgPool, template_id: Uuid) -> anyhow::Result<Vec<Prompt>> {
    let sql = format!(
        "SELECT {PROMPT_COLS} FROM mindset_prompts WHERE template_id = $1 \
         ORDER BY position, created_at"
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(template_id)
        .fetch_all(pool)
        .await
        .context("listing template prompts")?)
}

#[allow(clippy::too_many_arguments)]
pub async fn add_prompt(
    pool: &PgPool,
    template_id: Uuid,
    phase: &str,
    kind: &str,
    label: &str,
    hint: &str,
    config: &Value,
    position: f64,
) -> anyhow::Result<Prompt> {
    let sql = format!(
        "INSERT INTO mindset_prompts (id, template_id, phase, kind, label, hint, config, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {PROMPT_COLS}"
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(template_id)
        .bind(phase)
        .bind(kind)
        .bind(label)
        .bind(hint)
        .bind(config)
        .bind(position)
        .fetch_one(pool)
        .await
        .context("adding prompt")?)
}

/// Append position within a template (new prompts land at the end of their check-in).
pub async fn next_position(pool: &PgPool, template_id: Uuid) -> anyhow::Result<f64> {
    let (max,): (Option<f64>,) =
        sqlx::query_as("SELECT MAX(position) FROM mindset_prompts WHERE template_id = $1")
            .bind(template_id)
            .fetch_one(pool)
            .await
            .context("reading max prompt position")?;
    Ok(max.unwrap_or(0.0) + 1.0)
}

pub async fn update_prompt(
    pool: &PgPool,
    id: Uuid,
    label: Option<&str>,
    hint: Option<&str>,
    config: Option<&Value>,
    position: Option<f64>,
    active: Option<bool>,
) -> anyhow::Result<Option<Prompt>> {
    let sql = format!(
        "UPDATE mindset_prompts SET \
         label = COALESCE($2, label), \
         hint = COALESCE($3, hint), \
         config = COALESCE($4, config), \
         position = COALESCE($5, position), \
         active = COALESCE($6, active) \
         WHERE id = $1 RETURNING {PROMPT_COLS}"
    );
    Ok(sqlx::query_as::<_, Prompt>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(label)
        .bind(hint)
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

/// A prompt as the template form submits it: `id` present = keep that row (and the answers
/// already keyed to it); absent = insert.
#[derive(Debug, Clone, Default)]
pub struct NewPrompt {
    pub id: Option<Uuid>,
    pub kind: String,
    pub label: String,
    pub hint: String,
    pub config: Value,
}

/// Replace a template's prompts, keeping ids so past answers stay attached.
pub async fn set_prompts(
    pool: &PgPool,
    template_id: Uuid,
    phase: &str,
    prompts: &[NewPrompt],
) -> anyhow::Result<()> {
    let keep: Vec<Uuid> = prompts.iter().filter_map(|p| p.id).collect();
    sqlx::query("DELETE FROM mindset_prompts WHERE template_id = $1 AND NOT (id = ANY($2))")
        .bind(template_id)
        .bind(&keep)
        .execute(pool)
        .await
        .context("pruning prompts")?;
    for (i, p) in prompts.iter().enumerate() {
        match p.id {
            Some(id) => {
                sqlx::query(
                    "UPDATE mindset_prompts \
                     SET kind = $2, label = $3, hint = $4, config = $5, position = $6, phase = $7 \
                     WHERE id = $1 AND template_id = $8",
                )
                .bind(id)
                .bind(&p.kind)
                .bind(&p.label)
                .bind(&p.hint)
                .bind(&p.config)
                .bind(i as f64)
                .bind(phase)
                .bind(template_id)
                .execute(pool)
                .await
                .context("updating prompt")?;
            }
            None => {
                add_prompt(pool, template_id, phase, &p.kind, &p.label, &p.hint, &p.config, i as f64)
                    .await?;
            }
        }
    }
    Ok(())
}

// ── Day marks (consistency) ──────────────────────────────────────────────────

pub async fn list_marks(pool: &PgPool, from: Date, to: Date) -> anyhow::Result<Vec<DayMark>> {
    Ok(sqlx::query_as::<_, DayMark>(
        "SELECT day, mark FROM mindset_day_marks WHERE day BETWEEN $1 AND $2 ORDER BY day",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .context("listing day marks")?)
}

pub async fn get_mark(pool: &PgPool, day: Date) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>("SELECT mark FROM mindset_day_marks WHERE day = $1")
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
                "INSERT INTO mindset_day_marks (day, mark) VALUES ($1,$2) \
                 ON CONFLICT (day) DO UPDATE SET mark = EXCLUDED.mark, updated_at = now()",
            )
            .bind(day)
            .bind(m)
            .execute(pool)
            .await
            .context("setting day mark")?;
        }
        None => {
            sqlx::query("DELETE FROM mindset_day_marks WHERE day = $1")
                .bind(day)
                .execute(pool)
                .await
                .context("clearing day mark")?;
        }
    }
    Ok(())
}

// ── Entries ──────────────────────────────────────────────────────────────────

const ENTRY_COLS: &str = "id, entry_date, template_id, phase, answers";

pub async fn entries_for_date(pool: &PgPool, date: Date) -> anyhow::Result<Vec<Entry>> {
    let sql = format!("SELECT {ENTRY_COLS} FROM mindset_entries WHERE entry_date = $1");
    Ok(sqlx::query_as::<_, Entry>(sqlx::AssertSqlSafe(sql))
        .bind(date)
        .fetch_all(pool)
        .await
        .context("listing day entries")?)
}

/// Upsert the (date, template) check-in with a full answers map. `phase` is denormalised
/// from the template so history can bucket entries without a join.
pub async fn save_entry(
    pool: &PgPool,
    date: Date,
    template_id: Uuid,
    phase: &str,
    answers: &Value,
) -> anyhow::Result<Entry> {
    let sql = format!(
        "INSERT INTO mindset_entries (id, entry_date, template_id, phase, answers) \
         VALUES ($1,$2,$3,$4,$5) \
         ON CONFLICT (entry_date, template_id) \
         DO UPDATE SET answers = EXCLUDED.answers, phase = EXCLUDED.phase, updated_at = now() \
         RETURNING {ENTRY_COLS}"
    );
    Ok(sqlx::query_as::<_, Entry>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(date)
        .bind(template_id)
        .bind(phase)
        .bind(answers)
        .fetch_one(pool)
        .await
        .context("saving entry")?)
}

pub async fn delete_entry(pool: &PgPool, date: Date, template_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mindset_entries WHERE entry_date = $1 AND template_id = $2")
        .bind(date)
        .bind(template_id)
        .execute(pool)
        .await
        .context("deleting entry")?;
    Ok(())
}

/// Most recent entries (newest first), capped, for the history list and trend charts.
pub async fn recent_entries(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Entry>> {
    let sql = format!(
        "SELECT {ENTRY_COLS} FROM mindset_entries ORDER BY entry_date DESC LIMIT $1"
    );
    Ok(sqlx::query_as::<_, Entry>(sqlx::AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("listing recent entries")?)
}
