//! What every import in the app learns, whatever module it feeds.
//!
//! One table: the headers the user re-mapped by hand. It is scoped, because the same
//! header means different things to different modules — "Date" is a trade's entry to the
//! journal and an operation's date to a portfolio — and a vocabulary taught in one place
//! must not mis-map a file in another.

use anyhow::Context;
use sqlx::PgPool;
use std::collections::HashMap;

/// normalized header → target field, as taught by the user's past corrections in `scope`.
pub async fn load_aliases(pool: &PgPool, scope: &str) -> anyhow::Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT header, field FROM import_aliases WHERE scope = $1")
            .bind(scope)
            .fetch_all(pool)
            .await
            .context("loading import aliases")?;
    Ok(rows.into_iter().collect())
}

/// Remember (or reinforce) that `header` means `field` in `scope`.
pub async fn record_alias(
    pool: &PgPool,
    scope: &str,
    header: &str,
    field: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO import_aliases (scope, header, field) VALUES ($1, $2, $3) \
         ON CONFLICT (scope, header) DO UPDATE SET field = EXCLUDED.field, \
             hits = import_aliases.hits + 1, updated_at = now()",
    )
    .bind(scope)
    .bind(header)
    .bind(field)
    .execute(pool)
    .await
    .context("recording import alias")?;
    Ok(())
}
