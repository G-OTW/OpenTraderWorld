//! Social sign-in state: the provider configuration, the one linked identity, the
//! password-login switch and the recovery codes.
//!
//! The invariant the functions below keep: **password login can only be off while an
//! identity is linked.** Every path that removes the link ([`unlink`], [`delete`]) turns
//! it back on in the same transaction, so there is no state where the owner has no way in.

use anyhow::Context;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// The stored configuration and, once linked, the identity it is locked to.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SocialLogin {
    pub provider: String,
    /// Microsoft tenant, or the issuer URL of a generic OpenID Connect provider. Empty for
    /// Google and GitHub.
    pub issuer: String,
    pub client_id: String,
    pub secret_nonce: Option<Vec<u8>>,
    pub secret_ct: Option<Vec<u8>>,
    pub user_id: Option<Uuid>,
    pub subject_iss: Option<String>,
    pub subject: Option<String>,
    pub email: String,
    pub linked_at: Option<OffsetDateTime>,
}

impl SocialLogin {
    /// The identity this instance accepts, when one is linked.
    pub fn linked(&self) -> Option<(Uuid, &str, &str)> {
        match (self.user_id, self.subject_iss.as_deref(), self.subject.as_deref()) {
            (Some(u), Some(iss), Some(sub)) => Some((u, iss, sub)),
            _ => None,
        }
    }

    /// The sealed client secret, when one is stored.
    pub fn secret(&self) -> Option<(&[u8], &[u8])> {
        match (&self.secret_nonce, &self.secret_ct) {
            (Some(n), Some(c)) => Some((n.as_slice(), c.as_slice())),
            _ => None,
        }
    }
}

pub async fn get(pool: &PgPool) -> anyhow::Result<Option<SocialLogin>> {
    let row = sqlx::query_as::<_, SocialLogin>(
        "SELECT provider, issuer, client_id, secret_nonce, secret_ct, user_id, subject_iss, \
                subject, email, linked_at \
         FROM social_login WHERE id",
    )
    .fetch_optional(pool)
    .await
    .context("reading the social sign-in configuration")?;
    Ok(row)
}

/// Store the provider configuration. `secret` `None` keeps the stored one. The caller
/// refuses this while an identity is linked: a new provider or client would not issue the
/// same subject, so the link has to go first.
pub async fn save_config(
    pool: &PgPool,
    provider: &str,
    issuer: &str,
    client_id: &str,
    secret: Option<(&[u8], &[u8])>,
) -> anyhow::Result<()> {
    let (nonce, ct) = match secret {
        Some((n, c)) => (Some(n), Some(c)),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO social_login (id, provider, issuer, client_id, secret_nonce, secret_ct) \
         VALUES (TRUE, $1, $2, $3, $4, $5) \
         ON CONFLICT (id) DO UPDATE SET \
            provider = EXCLUDED.provider, issuer = EXCLUDED.issuer, \
            client_id = EXCLUDED.client_id, \
            secret_nonce = COALESCE(EXCLUDED.secret_nonce, social_login.secret_nonce), \
            secret_ct = COALESCE(EXCLUDED.secret_ct, social_login.secret_ct), \
            updated_at = now()",
    )
    .bind(provider)
    .bind(issuer)
    .bind(client_id)
    .bind(nonce)
    .bind(ct)
    .execute(pool)
    .await
    .context("saving the social sign-in configuration")?;
    Ok(())
}

/// Remove the client secret (a public client, or one that never needed it).
pub async fn clear_secret(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query("UPDATE social_login SET secret_nonce = NULL, secret_ct = NULL WHERE id")
        .execute(pool)
        .await
        .context("clearing the client secret")?;
    Ok(())
}

/// Lock sign-in to this identity.
pub async fn link(
    pool: &PgPool,
    user_id: Uuid,
    subject_iss: &str,
    subject: &str,
    email: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE social_login SET user_id = $1, subject_iss = $2, subject = $3, email = $4, \
                linked_at = now(), updated_at = now() WHERE id",
    )
    .bind(user_id)
    .bind(subject_iss)
    .bind(subject)
    .bind(email)
    .execute(pool)
    .await
    .context("linking the social identity")?;
    Ok(())
}

/// Drop the linked identity and turn password login back on. The configuration stays, so
/// linking another account is one click.
pub async fn unlink(pool: &PgPool, user_id: Uuid) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE social_login SET user_id = NULL, subject_iss = NULL, subject = NULL, \
                email = '', linked_at = NULL, updated_at = now() WHERE id",
    )
    .execute(&mut *tx)
    .await
    .context("unlinking the social identity")?;
    sqlx::query("UPDATE users SET password_login = TRUE WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Remove the configuration and the link, and turn password login back on.
pub async fn delete(pool: &PgPool, user_id: Uuid) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM social_login").execute(&mut *tx).await?;
    sqlx::query("UPDATE users SET password_login = TRUE WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await.context("removing social sign-in")?;
    Ok(())
}

// ── Password login switch ────────────────────────────────────────────────────

pub async fn password_login(pool: &PgPool, user_id: Uuid) -> anyhow::Result<bool> {
    let row: Option<(bool,)> = sqlx::query_as("SELECT password_login FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context("reading the password-login switch")?;
    Ok(row.map(|r| r.0).unwrap_or(true))
}

/// Turn password login on or off. Off is refused here too, not only in the handler, unless
/// this account holds the linked identity: the invariant lives next to the data.
pub async fn set_password_login(
    pool: &PgPool,
    user_id: Uuid,
    enabled: bool,
) -> anyhow::Result<bool> {
    let done = sqlx::query(
        "UPDATE users SET password_login = $2 WHERE id = $1 \
           AND ($2 OR EXISTS (SELECT 1 FROM social_login \
                              WHERE user_id = $1 AND subject IS NOT NULL))",
    )
    .bind(user_id)
    .bind(enabled)
    .execute(pool)
    .await
    .context("switching password login")?;
    Ok(done.rows_affected() == 1)
}

// ── Recovery codes ───────────────────────────────────────────────────────────

/// Replace every recovery code of this account with a fresh set (hashes only).
pub async fn replace_recovery_codes(
    pool: &PgPool,
    user_id: Uuid,
    hashes: &[String],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for hash in hashes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(hash)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await.context("storing recovery codes")?;
    Ok(())
}

pub async fn recovery_codes_left(pool: &PgPool, user_id: Uuid) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .context("counting recovery codes")?;
    Ok(row.0)
}

/// Spend one code. True when it existed and was unused; one statement, so two requests
/// racing with the same code cannot both win.
pub async fn consume_recovery_code(
    pool: &PgPool,
    user_id: Uuid,
    hash: &str,
) -> anyhow::Result<bool> {
    let done = sqlx::query(
        "UPDATE recovery_codes SET used_at = now() \
         WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(user_id)
    .bind(hash)
    .execute(pool)
    .await
    .context("spending a recovery code")?;
    Ok(done.rows_affected() == 1)
}

/// Whether a code is live, without spending it. A sign-in that still owes the second
/// factor checks this first, so a missing authenticator code does not cost a code.
pub async fn recovery_code_valid(
    pool: &PgPool,
    user_id: Uuid,
    hash: &str,
) -> anyhow::Result<bool> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM recovery_codes \
                       WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL)",
    )
    .bind(user_id)
    .bind(hash)
    .fetch_one(pool)
    .await
    .context("checking a recovery code")?;
    Ok(row.0)
}
