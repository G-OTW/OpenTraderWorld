//! Durable persistence over PostgreSQL (registry, jobs, notes/doc page).
//!
//! Phase 0/3: connection pool, schema migration, and the user/session queries the
//! first-run wizard and auth need. Module/job/notes tables land in later phases.

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

pub mod api_rate;
pub mod backtest;
pub mod calendar;
pub mod community_docs;
pub mod crypto;
pub mod data_admin;
pub mod databases;
pub mod documents;
pub mod feeds;
pub mod files;
pub mod findb;
pub mod goals;
pub mod histdata;
pub mod journal;
pub mod journal_fx;
pub mod logs;
pub mod mcp;
pub mod mindset;
pub mod mportfolios;
pub mod notif_channels;
pub mod portfolios;
pub mod reminders;
pub mod resources;
pub mod settings;
pub mod subscriptions;
pub mod taxcalc;
pub mod todos;
pub mod trader_tasks;
pub mod time_tracker;
pub mod wealth;

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

/// Store a session token for a user, expiring `ttl_hours` from now.
pub async fn create_session(
    pool: &PgPool,
    token: &str,
    user_id: Uuid,
    ttl_hours: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at) \
         VALUES ($1, $2, now() + ($3 || ' hours')::interval)",
    )
    .bind(token)
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
         WHERE s.token = $1 AND s.expires_at > now()",
    )
    .bind(token)
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
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(token)
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
