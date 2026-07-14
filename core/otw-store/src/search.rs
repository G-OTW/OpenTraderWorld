//! Global search — title/name-only lookups across module content tables.
//!
//! One UNION ALL query assembled from the fixed per-scope fragments below. Fragments are
//! static strings (never user input), so `AssertSqlSafe` holds; the user's query only
//! ever travels through bind parameters ($1 contains-pattern, $2 prefix-pattern).

use serde::Serialize;
use sqlx::{AssertSqlSafe, PgPool};

/// Rows returned per scope; the top-bar dropdown shows a handful per type.
const PER_SCOPE_LIMIT: &str = "8";

/// One hit: `kind` echoes the scope key; `sub` is light context (category, tags, date).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub sub: String,
}

/// Scope key → SELECT fragment yielding (kind, id, name, sub) plus the column to rank
/// on. Prefix matches sort first, then alphabetical.
const SCOPES: &[(&str, &str, &str)] = &[
    (
        "resources",
        "SELECT 'resources'::text AS kind, r.id::text AS id, r.name AS name, c.name AS sub \
         FROM resources r JOIN resource_categories c ON c.id = r.category_id \
         WHERE r.name ILIKE $1",
        "r.name",
    ),
    (
        "documents",
        "SELECT 'documents', id::text, title, '' FROM documents \
         WHERE kind = 'page' AND title ILIKE $1",
        "title",
    ),
    (
        "goals",
        "SELECT 'goals', id::text, name, '' FROM goals WHERE name ILIKE $1",
        "name",
    ),
    (
        "events",
        "SELECT 'events', id::text, title, to_char(start_at, 'YYYY-MM-DD') \
         FROM calendar_events WHERE title ILIKE $1",
        "title",
    ),
    (
        "todos",
        "SELECT 'todos', id::text, name, '' FROM todos WHERE name ILIKE $1",
        "name",
    ),
    (
        "routines",
        "SELECT 'routines', id::text, name, '' FROM trader_routines WHERE name ILIKE $1",
        "name",
    ),
    (
        "reminders",
        "SELECT 'reminders', id::text, name, '' FROM reminders WHERE name ILIKE $1",
        "name",
    ),
    (
        "prompts",
        "SELECT 'prompts', id::text, name, array_to_string(tags, ', ') \
         FROM prompt_store_prompts WHERE name ILIKE $1",
        "name",
    ),
    (
        "community-docs",
        "SELECT 'community-docs', id::text, title, array_to_string(categories, ', ') \
         FROM community_docs WHERE title ILIKE $1",
        "title",
    ),
];

/// All valid scope keys (the API layer filters requests against this).
pub fn known_scopes() -> impl Iterator<Item = &'static str> {
    SCOPES.iter().map(|(key, _, _)| *key)
}

/// Escape LIKE metacharacters so the user's text matches literally.
fn escape_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Title-only search over the requested scopes (unknown keys are skipped). Results come
/// back grouped in `SCOPES` order, at most `PER_SCOPE_LIMIT` per scope.
pub async fn search_titles(
    pool: &PgPool,
    scopes: &[&str],
    q: &str,
) -> anyhow::Result<Vec<SearchHit>> {
    let fragments: Vec<String> = SCOPES
        .iter()
        .filter(|(key, _, _)| scopes.contains(key))
        .map(|(_, select, rank_col)| {
            format!(
                "({select} ORDER BY ({rank_col} ILIKE $2) DESC, lower({rank_col}) \
                 LIMIT {PER_SCOPE_LIMIT})"
            )
        })
        .collect();
    if fragments.is_empty() {
        return Ok(Vec::new());
    }

    let escaped = escape_like(q);
    let rows = sqlx::query_as::<_, SearchHit>(AssertSqlSafe(fragments.join(" UNION ALL ")))
        .bind(format!("%{escaped}%"))
        .bind(format!("{escaped}%"))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}
