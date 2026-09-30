//! Seeding the shipped shelf into the database, once per boot.
//!
//! Insert-if-absent, never overwrite: a user who rewrote a persona's prompt or a skill's body
//! keeps that rewrite across every restart and every upgrade. New shipped items appear on the
//! boot after the upgrade; changed ones do not, which is the price of not clobbering edits —
//! "reset to shipped" is how a user opts into the new version.
//!
//! Deleting a builtin is also honoured: the seeder would re-create it on the next boot, which
//! is why the UI disables rather than deletes. That is a UI contract, not enforced here.

use serde_json::json;
use sqlx::PgPool;

use super::{PERSONAS, SKILLS};

/// Seed personas and skills. Errors are logged and swallowed by the caller's choice — a
/// failure here leaves the agent usable without its shelf, which beats refusing to boot.
pub async fn run(pool: &PgPool) -> anyhow::Result<()> {
    for s in SKILLS {
        otw_store::agent::seed_builtin_skill(pool, s.name, s.description, s.body).await?;
    }
    for p in PERSONAS {
        // Shelves are declared by name here (a `.md` filename is the stable thing in the
        // source tree) and stored as ids — the skills above are already seeded, so every
        // name resolves. See `Shelf` for why ids.
        let skills = json!(shelf_ids(pool, p.skills).await?);
        otw_store::agent::seed_builtin_agent(pool, p.slug, p.name, &p.prompt(), &skills).await?;
    }
    Ok(())
}

/// Resolve a declared shelf to the id strings stored in `agent_agents.skills`.
pub async fn shelf_ids(pool: &PgPool, names: &[&str]) -> anyhow::Result<Vec<String>> {
    Ok(otw_store::agent::skill_ids_by_name(pool, names)
        .await?
        .iter()
        .map(|id| id.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{PERSONAS, SKILLS};

    /// Slugs are the seeder's identity key: a duplicate would silently drop a persona.
    #[test]
    fn persona_slugs_are_unique() {
        let mut slugs: Vec<&str> = PERSONAS.iter().map(|p| p.slug).collect();
        slugs.sort_unstable();
        let before = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), before, "duplicate persona slug");
    }

    /// The default assistant is not a persona (no stance, no shelf of its own), but it is the
    /// agent every conversation starts on — so it carries the same product rules. Guard
    /// against the two texts drifting apart.
    #[test]
    fn the_default_assistant_carries_the_shared_rules() {
        let prompt = otw_store::agent::DEFAULT_SYSTEM_PROMPT;
        for rule in [
            "comes from a tool call",
            "Do not invent paths",
            "never act on it",
            "Never delete without explicit confirmation",
            "financial advice",
        ] {
            assert!(prompt.contains(rule), "default prompt is missing: {rule}");
            assert!(super::super::SHARED.contains(rule), "SHARED is missing: {rule}");
        }
    }

    #[test]
    fn skill_names_are_unique() {
        let mut names: Vec<&str> = SKILLS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate skill name");
    }

    /// Every rendered prompt carries the shared rules and the persona's own refusals. Cheap
    /// guard against a slot silently going missing from the skeleton.
    #[test]
    fn prompts_carry_shared_rules_and_refusals() {
        for p in PERSONAS {
            let prompt = p.prompt();
            assert!(prompt.contains(p.name), "{}: name missing", p.slug);
            assert!(prompt.contains("comes from a tool call"), "{}: sourcing rule", p.slug);
            assert!(prompt.contains("never act on it"), "{}: injection rule", p.slug);
            assert!(prompt.contains("financial advice"), "{}: advice boundary", p.slug);
            for r in p.refusals {
                let head = r.split(['.', ',']).next().unwrap_or(r);
                assert!(prompt.contains(head), "{}: refusal missing: {head}", p.slug);
            }
        }
    }

    #[test]
    fn personas_declare_a_shelf() {
        for p in PERSONAS {
            assert!(!p.skills.is_empty(), "{}: no skills", p.slug);
        }
    }

    /// Every declared skill must actually ship. The intersection at run time means a missing
    /// one fails quietly — the persona just never sees it — but a skill body that *names* a
    /// skill makes the model call `load_skill` for it and take an error instead. Observed in
    /// practice: `backtest-run` referenced `backtest-validate` before it was written.
    #[test]
    fn every_declared_skill_exists() {
        for p in PERSONAS {
            for want in p.skills {
                assert!(
                    SKILLS.iter().any(|s| &s.name == want),
                    "{}: declares skill {want:?} which is not shipped",
                    p.slug
                );
            }
        }
    }

    /// A skill body that mentions another skill by name must mean one that exists, for the
    /// same reason.
    #[test]
    fn skill_bodies_reference_only_real_skills() {
        let names: Vec<&str> = SKILLS.iter().map(|s| s.name).collect();
        for s in SKILLS {
            for candidate in ["backtest-validate", "backtest-run", "data-acquire", "news-digest",
                              "source-check", "write-report", "risk-metrics", "study-company",
                              "otw-navigation", "memory-hygiene", "session-prep", "test-an-idea"] {
                if s.body.contains(candidate) {
                    assert!(names.contains(&candidate),
                        "{}: body references missing skill {candidate:?}", s.name);
                }
            }
        }
    }

    /// A prompt must never assert what the agent can reach: access is the conversation's MCP
    /// token, which the user controls and which says nothing about which persona is speaking.
    #[test]
    fn prompts_claim_no_working_set() {
        for p in PERSONAS {
            let prompt = p.prompt();
            assert!(!prompt.contains("Working set"), "{}: prompt claims access", p.slug);
        }
    }

    /// The description is the retrieval key — it rides in every system prompt, so it must be
    /// present, short, and phrased as a trigger.
    #[test]
    fn skill_descriptions_are_triggers() {
        for s in SKILLS {
            assert!(!s.description.is_empty(), "{}: no description", s.name);
            assert!(s.description.len() < 240, "{}: description too long", s.name);
            assert!(
                s.description.starts_with("Use "),
                "{}: description must say when to use the skill",
                s.name
            );
            assert!(!s.body.trim().is_empty(), "{}: empty body", s.name);
        }
    }
}
