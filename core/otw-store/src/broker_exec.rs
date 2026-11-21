//! E2D execution (onboarding spec 9): the per-account acknowledgement that a broker's order
//! path was never tested against its venue.

use anyhow::Context;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// The acknowledgement on an account, when it was signed.
pub async fn untested_ack(pool: &PgPool, account: Uuid) -> anyhow::Result<Option<OffsetDateTime>> {
    let row: Option<(OffsetDateTime,)> = sqlx::query_as("SELECT acknowledged_at FROM broker_live_untested WHERE account_id = $1")
        .bind(account)
        .fetch_optional(pool)
        .await
        .context("reading the untested-broker acknowledgement")?;
    Ok(row.map(|r| r.0))
}

/// Store the acknowledgement, in the words the account owner was shown.
pub async fn sign_untested(pool: &PgPool, account: Uuid, statement: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO broker_live_untested (account_id, statement) VALUES ($1, $2) \
         ON CONFLICT (account_id) DO UPDATE SET statement = EXCLUDED.statement, acknowledged_at = now()",
    )
    .bind(account)
    .bind(statement)
    .execute(pool)
    .await
    .context("storing the untested-broker acknowledgement")?;
    Ok(())
}

/// Drop the acknowledgement (live trading turned off for the account).
pub async fn drop_untested(pool: &PgPool, account: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM broker_live_untested WHERE account_id = $1")
        .bind(account)
        .execute(pool)
        .await
        .context("dropping the untested-broker acknowledgement")?;
    Ok(())
}
