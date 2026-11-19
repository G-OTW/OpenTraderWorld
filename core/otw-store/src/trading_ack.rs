//! Background trading signatures (`trading_acknowledgments`): one row per time the user
//! signed the permission for orders sent with nobody watching, on one broker account.
//!
//! The table is append-only in the database (a trigger refuses UPDATE and DELETE, except the
//! account's deletion detaching its rows). Turning the permission off only clears the
//! account's pointer (`broker_accounts.background_trading_ack`), so the history keeps every
//! signature, the ones no longer in force included. Turning it on again is a new row: a
//! signature is never reused.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// A signature, as it is written.
pub struct NewAck<'a> {
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub account_name: &'a str,
    pub broker: &'a str,
    /// `paper`, `sandbox` or `live`.
    pub environment: &'a str,
    pub terms_version: &'a str,
    pub terms: &'a serde_json::Value,
    pub statements: &'a serde_json::Value,
    pub locale: &'a str,
    pub typed: &'a str,
    pub second_factor: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Ack {
    pub id: i64,
    pub user_id: Uuid,
    /// `None` once the account was deleted.
    pub account_id: Option<Uuid>,
    pub account_name: String,
    pub broker: String,
    pub environment: String,
    pub terms_version: String,
    pub terms: serde_json::Value,
    pub statements: serde_json::Value,
    pub locale: String,
    pub typed: String,
    pub second_factor: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub signed_at: OffsetDateTime,
}

const COLS: &str = "id, user_id, account_id, account_name, broker, environment, terms_version, \
                    terms, statements, locale, typed, second_factor, signed_at";

/// Write the signature and make it the one in force on its account, in one transaction: a
/// row is never left pointing nowhere, nor an account pointing at a row that was not written.
pub async fn sign(pool: &PgPool, a: &NewAck<'_>) -> anyhow::Result<Ack> {
    let mut tx = pool.begin().await?;
    let sql = format!(
        "INSERT INTO trading_acknowledgments (user_id, account_id, account_name, broker, environment, \
         terms_version, terms, statements, locale, typed, second_factor) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING {COLS}"
    );
    let ack = sqlx::query_as::<_, Ack>(sqlx::AssertSqlSafe(sql))
        .bind(a.user_id)
        .bind(a.account_id)
        .bind(a.account_name)
        .bind(a.broker)
        .bind(a.environment)
        .bind(a.terms_version)
        .bind(a.terms)
        .bind(a.statements)
        .bind(a.locale)
        .bind(a.typed)
        .bind(a.second_factor)
        .fetch_one(&mut *tx)
        .await
        .context("recording the background trading signature")?;
    sqlx::query("UPDATE broker_accounts SET background_trading_ack = $2, updated_at = now() WHERE id = $1")
        .bind(a.account_id)
        .bind(ack.id)
        .execute(&mut *tx)
        .await
        .context("turning background trading on")?;
    tx.commit().await?;
    Ok(ack)
}

/// Turn background trading off for an account. The signature stays in the history. Returns
/// whether it was on.
pub async fn revoke(pool: &PgPool, account_id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE broker_accounts SET background_trading_ack = NULL, updated_at = now() \
         WHERE id = $1 AND background_trading_ack IS NOT NULL",
    )
    .bind(account_id)
    .execute(pool)
    .await
    .context("turning background trading off")?;
    Ok(res.rows_affected() > 0)
}

/// The signature in force on an account, if any.
pub async fn current(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Option<Ack>> {
    let sql = format!(
        "SELECT {COLS} FROM trading_acknowledgments \
         WHERE id = (SELECT background_trading_ack FROM broker_accounts WHERE id = $1)"
    );
    Ok(sqlx::query_as::<_, Ack>(sqlx::AssertSqlSafe(sql)).bind(account_id).fetch_optional(pool).await?)
}

/// Every signature made on an account, newest first, the ones no longer in force included.
pub async fn history(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<Ack>> {
    let sql = format!("SELECT {COLS} FROM trading_acknowledgments WHERE account_id = $1 ORDER BY signed_at DESC, id DESC");
    Ok(sqlx::query_as::<_, Ack>(sqlx::AssertSqlSafe(sql)).bind(account_id).fetch_all(pool).await?)
}
