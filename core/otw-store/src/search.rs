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
        "mailbox",
        "SELECT 'mailbox', m.id::text, m.subject, COALESCE(NULLIF(s.name, ''), s.from_addr) \
         FROM mailbox_messages m JOIN mailbox_senders s ON s.id = m.sender_id \
         WHERE m.subject ILIKE $1",
        "m.subject",
    ),
    (
        "community-docs",
        "SELECT 'community-docs', id::text, title, array_to_string(categories, ', ') \
         FROM community_docs WHERE title ILIKE $1",
        "title",
    ),
    (
        "fund-companies",
        "SELECT 'fund-companies', ticker, name, ticker FROM fund_companies \
         WHERE name ILIKE $1 OR ticker ILIKE $1",
        "name",
    ),
    (
        "fund-series",
        "SELECT 'fund-series', id::text, title, upper(provider) || ' ' || code FROM fund_series \
         WHERE title ILIKE $1 OR code ILIKE $1",
        "title",
    ),
    (
        // The id carries the company and the tab that lists the document.
        "fund-documents",
        "SELECT 'fund-documents', \
                c.ticker || ':' || CASE WHEN d.kind = 'transcript' THEN 'transcripts' ELSE 'filings' END, \
                d.title, c.ticker || ' · ' || d.form || ' · ' || to_char(d.filed_at, 'YYYY-MM-DD') \
         FROM fund_documents d JOIN fund_companies c ON c.id = d.company_id \
         WHERE d.title ILIKE $1",
        "d.title",
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
    // A UNION takes its column names from its first branch, and only some fragments alias
    // theirs: name them once outside, whichever scope comes first.
    let sql = format!(
        "SELECT kind, id, name, sub FROM ({}) AS hits(kind, id, name, sub)",
        fragments.join(" UNION ALL ")
    );
    let rows = sqlx::query_as::<_, SearchHit>(AssertSqlSafe(sql))
        .bind(format!("%{escaped}%"))
        .bind(format!("{escaped}%"))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}
