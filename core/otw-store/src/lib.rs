//! Durable persistence over PostgreSQL (registry, jobs, notes/doc page).
//!
//! Phase 0/3: connection pool, schema migration, and the user/session queries the
//! first-run wizard and auth need. Module/job/notes tables land in later phases.

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

pub mod agent;
pub mod api_quota;
pub mod automator;
pub mod api_rate;
pub mod backtest;
pub mod calendar;
pub mod community_docs;
pub mod connectors;
pub mod crypto;
pub mod data_admin;
pub mod databases;
pub mod documents;
pub mod feeds;
pub mod files;
pub mod findb;
pub mod goals;
pub mod histdata;
pub mod histviz;
pub mod imports;
pub mod journal;
pub mod journal_analytics;
pub mod journal_behavior;
pub mod journal_exposure;
pub mod journal_fx;
pub mod journal_import;
pub mod journal_market;
pub mod journal_routines;
pub mod logs;
pub mod mailbox;
pub mod mcp;
pub mod mindset;
pub mod mportfolios;
pub mod notif_channels;
pub mod paper;
pub mod portfolios;
pub mod portfolios_import;
pub mod portfolios_risk;
pub mod prompts;
pub mod reminders;
pub mod resources;
pub mod search;
pub mod settings;
pub mod subscriptions;
pub mod taxcalc;
pub mod todos;
pub mod trader_tasks;
pub mod time_tracker;
pub mod vault;
pub mod watchlist_alerts;
pub mod watchlists;
pub mod wealth;
pub mod webhooks;

/// A persisted user row.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    /// True while the account still has its bootstrap password and must set a new one before
    /// using the app (see the headless install path). Cleared on the first password change.
    pub must_change_password: bool,
}

/// Connect to Postgres (with a bounded pool) and run migrations.
pub async fn connect_and_migrate(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .context("connecting to Postgres")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running migrations")?;

    Ok(pool)
}

/// True once at least one admin account exists (drives the setup-state check).
pub async fn admin_exists(pool: &PgPool) -> anyhow::Result<bool> {
    let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE is_admin)")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Create the admin account. Caller supplies an already-hashed password. `must_change` marks
/// the account to force a password change on first login (used by the headless bootstrap,
/// where the password was auto-generated rather than chosen).
pub async fn create_admin(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
    must_change: bool,
) -> anyhow::Result<User> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, is_admin, must_change_password) \
         VALUES ($1, $2, $3, TRUE, $4)",
    )
    .bind(id)
    .bind(username)
    .bind(password_hash)
    .bind(must_change)
    .execute(pool)
    .await
    .context("inserting admin user")?;
    Ok(User {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        is_admin: true,
        must_change_password: must_change,
    })
}

/// The admin account, if any (demo mode signs every visitor in as it).
pub async fn first_admin(pool: &PgPool) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password \
         FROM users WHERE is_admin ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// Look up a user by username (for login).
pub async fn find_user_by_username(
    pool: &PgPool,
    username: &str,
) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password \
         FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// Build a `User` from the canonical column tuple used by every user query.
fn user_from_row(
    (id, username, password_hash, is_admin, must_change_password): (Uuid, String, String, bool, bool),
) -> User {
    User { id, username, password_hash, is_admin, must_change_password }
}

/// The single admin user (MCP dispatch impersonates it — single-user app).
pub async fn find_admin(pool: &PgPool) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password \
         FROM users WHERE is_admin LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// Store a session token for a user, expiring `ttl_hours` from now. Expired sessions are
/// purged on the way in — logins are rare, so this keeps the table bounded without a
/// dedicated background task.
///
/// Only the SHA-256 of the token is persisted: the plaintext lives in the user's cookie and
/// nowhere else, so a database copy carries no replayable session. Same treatment as the MCP
/// and webhook tokens ([`crate::mcp::hash_token`]).
pub async fn create_session(
    pool: &PgPool,
    token: &str,
    user_id: Uuid,
    ttl_hours: i64,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) \
         VALUES ($1, $2, now() + ($3 || ' hours')::interval)",
    )
    .bind(crate::mcp::hash_token(token))
    .bind(user_id)
    .bind(ttl_hours.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolve a session token to its (non-expired) user, if any.
pub async fn user_for_session(pool: &PgPool, token: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool)>(
        "SELECT u.id, u.username, u.password_hash, u.is_admin, u.must_change_password \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.expires_at > now()",
    )
    .bind(crate::mcp::hash_token(token))
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// Look up a user by id.
pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// Delete a single session (logout).
pub async fn delete_session(pool: &PgPool, token: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(crate::mcp::hash_token(token))
        .execute(pool)
        .await?;
    Ok(())
}

/// Set a user's username and/or password hash. Pass `None` to leave a field unchanged.
/// Updating the password also revokes all of that user's other sessions.
pub async fn update_credentials(
    pool: &PgPool,
    user_id: Uuid,
    new_username: Option<&str>,
    new_password_hash: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(username) = new_username {
        sqlx::query("UPDATE users SET username = $2 WHERE id = $1")
            .bind(user_id)
            .bind(username)
            .execute(pool)
            .await
            .context("updating username")?;
    }
    if let Some(hash) = new_password_hash {
        // Setting a password also clears the force-change flag: the operator has now chosen
        // their own password, so the bootstrap credential no longer applies.
        sqlx::query("UPDATE users SET password_hash = $2, must_change_password = FALSE WHERE id = $1")
            .bind(user_id)
            .bind(hash)
            .execute(pool)
            .await
            .context("updating password")?;
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .context("revoking sessions after password change")?;
    }
    Ok(())
}

/// Out-of-band password reset, driven from the host shell (see otw-core's `cli` module).
///
/// Deliberately not [`update_credentials`]: that one *clears* the force-change flag because
/// the user typed their own password into the app, whereas this password was printed on a
/// terminal or piped through a shell. So it sets the flag instead — the value buys one
/// sign-in and the app demands a real password straight after — and drops every session, so
/// a cookie stolen before the reset cannot survive it.
pub async fn reset_password(
    pool: &PgPool,
    user_id: Uuid,
    password_hash: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE users SET password_hash = $2, must_change_password = TRUE WHERE id = $1",
    )
    .bind(user_id)
    .bind(password_hash)
    .execute(pool)
    .await
    .context("resetting password")?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .context("revoking sessions after password reset")?;
    Ok(())
}

/// Every account name, oldest first. Used by the host CLI when the operator has forgotten
/// which username they picked.
pub async fn list_usernames(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT username FROM users ORDER BY created_at")
            .fetch_all(pool)
            .await
            .context("listing usernames")?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
