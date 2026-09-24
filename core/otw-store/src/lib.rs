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
pub mod brokers;
pub mod calendar;
pub mod community_docs;
pub mod connectors;
pub mod control;
pub mod crypto;
pub mod data_admin;
pub mod data_transfer;
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
    /// True once a TOTP secret is enrolled *and* confirmed by a correct code. A stored
    /// secret with this flag still false is a half-finished enrolment and is ignored by
    /// the login path.
    pub totp_enabled: bool,
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
        totp_enabled: false,
    })
}

/// The admin account, if any (demo mode signs every visitor in as it).
pub async fn first_admin(pool: &PgPool) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password, totp_enabled \
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
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password, totp_enabled \
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
    (id, username, password_hash, is_admin, must_change_password, totp_enabled): (
        Uuid,
        String,
        String,
        bool,
        bool,
        bool,
    ),
) -> User {
    User { id, username, password_hash, is_admin, must_change_password, totp_enabled }
}

/// The single admin user (MCP dispatch impersonates it — single-user app).
pub async fn find_admin(pool: &PgPool) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password, totp_enabled \
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
///
/// `ip` and `user_agent` are provenance for the active-sessions list in Settings. They are
/// display strings, never authorization input: nothing is decided from them.
pub async fn create_session(
    pool: &PgPool,
    token: &str,
    user_id: Uuid,
    ttl_hours: i64,
    ip: &str,
    user_agent: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at, ip, user_agent) \
         VALUES ($1, $2, now() + ($3 || ' hours')::interval, $4, $5)",
    )
    .bind(crate::mcp::hash_token(token))
    .bind(user_id)
    .bind(ttl_hours.to_string())
    .bind(ip)
    // A header is attacker-controlled text; cap it so one request cannot store a novel.
    .bind(user_agent.chars().take(300).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolve a session token to its user, if the session is live.
///
/// Two clocks bound a session. `expires_at` is the absolute ceiling set at sign-in and never
/// moves. `idle_hours`, when given, adds a sliding one: a session untouched for that long is
/// dead even though its absolute expiry is days away. Passing `None` keeps the absolute
/// ceiling as the only rule, which is what a localhost-only install wants.
///
/// The row's `last_seen_at` is pushed forward on every successful resolution, which is both
/// what feeds the sliding window and what the sessions list displays.
pub async fn user_for_session(
    pool: &PgPool,
    token: &str,
    idle_hours: Option<i64>,
) -> anyhow::Result<Option<User>> {
    let hash = crate::mcp::hash_token(token);
    // One statement: touch the row and return its user, so a live session cannot be read as
    // live and then written as stale by a concurrent request.
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool, bool)>(
        "UPDATE sessions s SET last_seen_at = now() \
         FROM users u \
         WHERE u.id = s.user_id \
           AND s.token_hash = $1 \
           AND s.expires_at > now() \
           AND ($2::bigint IS NULL OR s.last_seen_at > now() - ($2 || ' hours')::interval) \
         RETURNING u.id, u.username, u.password_hash, u.is_admin, u.must_change_password, \
                   u.totp_enabled",
    )
    .bind(hash)
    .bind(idle_hours)
    .fetch_optional(pool)
    .await?
    .map(user_from_row);
    Ok(user)
}

/// One row of the active-sessions list.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SessionInfo {
    /// The session's SHA-256 token hash. It is the row's identity and safe to hand to the
    /// browser: it is a digest of a secret, not the secret, and knowing it grants nothing.
    pub id: String,
    pub created_at: time::OffsetDateTime,
    pub last_seen_at: time::OffsetDateTime,
    pub expires_at: time::OffsetDateTime,
    pub ip: String,
    pub user_agent: String,
}

/// Every live session for a user, most recently active first.
pub async fn list_sessions(pool: &PgPool, user_id: Uuid) -> anyhow::Result<Vec<SessionInfo>> {
    let rows = sqlx::query_as::<_, SessionInfo>(
        "SELECT token_hash AS id, created_at, last_seen_at, expires_at, ip, user_agent \
         FROM sessions WHERE user_id = $1 AND expires_at > now() \
         ORDER BY last_seen_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("listing sessions")?;
    Ok(rows)
}

/// Revoke one session of this user by its id (the token hash from [`list_sessions`]).
/// Scoped to the user so an id from elsewhere cannot be used to close a stranger's session.
pub async fn revoke_session(pool: &PgPool, user_id: Uuid, id: &str) -> anyhow::Result<u64> {
    let done = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash = $2")
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await
        .context("revoking a session")?;
    Ok(done.rows_affected())
}

/// Revoke every session of this user except the one presenting `keep_token`. This is the
/// "sign out everywhere else" button: the person clicking it keeps working.
pub async fn revoke_other_sessions(
    pool: &PgPool,
    user_id: Uuid,
    keep_token: &str,
) -> anyhow::Result<u64> {
    let done = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash <> $2")
        .bind(user_id)
        .bind(crate::mcp::hash_token(keep_token))
        .execute(pool)
        .await
        .context("revoking other sessions")?;
    Ok(done.rows_affected())
}

/// Record that this account signed in from `ip`, and say whether that source is new.
///
/// `true` means no successful sign-in has ever come from this address, which is what the
/// caller turns into a notification. The row is written either way, so the alert fires once
/// per source rather than on every sign-in. An empty or unresolvable address (a direct hit
/// with no proxy header) is never called new: it would alert on every login of a
/// localhost-only install.
pub async fn note_login_source(pool: &PgPool, user_id: Uuid, ip: &str) -> anyhow::Result<bool> {
    let ip = ip.trim();
    if ip.is_empty() || ip == "direct" {
        return Ok(false);
    }
    let row: (bool,) = sqlx::query_as(
        "INSERT INTO login_ips (user_id, ip) VALUES ($1, $2) \
         ON CONFLICT (user_id, ip) DO UPDATE SET last_seen = now() \
         RETURNING (login_ips.first_seen = login_ips.last_seen) AS is_new",
    )
    .bind(user_id)
    .bind(ip)
    .fetch_one(pool)
    .await
    .context("recording the login source")?;
    Ok(row.0)
}

// ── Second factor ────────────────────────────────────────────────────────────

/// Store a sealed TOTP secret against an account, leaving it *disabled*.
///
/// Enrolment is deliberately two steps. The secret has to be stored for the QR code to mean
/// anything, but a secret the user never managed to scan must not become a login
/// requirement: only [`enable_totp`], called after a correct code, flips the flag. Replacing
/// a pending secret is allowed; the confirmed one is protected by the caller.
pub async fn set_totp_secret(
    pool: &PgPool,
    user_id: Uuid,
    nonce: &[u8],
    ciphertext: &[u8],
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE users SET totp_nonce = $2, totp_secret = $3, totp_enabled = FALSE WHERE id = $1",
    )
    .bind(user_id)
    .bind(nonce)
    .bind(ciphertext)
    .execute(pool)
    .await
    .context("storing the TOTP secret")?;
    Ok(())
}

/// The sealed TOTP secret of an account, if one is stored (enrolled or merely pending).
pub async fn totp_secret(
    pool: &PgPool,
    user_id: Uuid,
) -> anyhow::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let row: Option<(Option<Vec<u8>>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT totp_nonce, totp_secret FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .context("reading the TOTP secret")?;
    Ok(row.and_then(|(n, c)| Some((n?, c?))))
}

/// Confirm an enrolment: from here on the account needs a code to sign in.
pub async fn enable_totp(pool: &PgPool, user_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET totp_enabled = TRUE WHERE id = $1 AND totp_secret IS NOT NULL")
        .bind(user_id)
        .execute(pool)
        .await
        .context("enabling TOTP")?;
    Ok(())
}

/// Remove the second factor and its secret. Called from Settings (with the password and a
/// current code) and from the host CLI (the recovery path for a lost authenticator).
pub async fn disable_totp(pool: &PgPool, user_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE users SET totp_enabled = FALSE, totp_secret = NULL, totp_nonce = NULL \
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .context("disabling TOTP")?;
    Ok(())
}

/// Look up a user by id.
pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, (Uuid, String, String, bool, bool, bool)>(
        "SELECT id, username, password_hash, is_admin, must_change_password, totp_enabled \
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

/// Drop every session of a user. The blunt form of [`revoke_other_sessions`], used by the
/// host-CLI recovery paths where there is no current session to preserve.
pub async fn revoke_all_sessions(pool: &PgPool, user_id: Uuid) -> anyhow::Result<u64> {
    let done = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .context("revoking every session")?;
    Ok(done.rows_affected())
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
