//! Per-instrument facts a script reads through `syminfo.*` (tick size, exchange timezone, …),
//! keyed by instrument rather than dataset: the chart also draws instruments it never stored.
//! `None` = unknown; nothing here is ever defaulted.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow, schemars::JsonSchema)]
pub struct InstrumentMeta {
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    #[serde(default)]
    pub mintick: Option<f64>,
    #[serde(default)]
    pub pointvalue: Option<f64>,
    /// IANA zone the exchange's sessions are delimited in (`America/New_York`).
    #[serde(default)]
    pub timezone: Option<String>,
    /// Pine session string: `0930-1600`, `0930-1600:23456`, `24x7`.
    #[serde(default)]
    pub session: Option<String>,
    /// `syminfo.type`: stock, crypto, forex, futures, index, …
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub basecurrency: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// A future's initial margin per contract, instrument currency: what a paper account sets
    /// aside to hold one.
    #[serde(default)]
    pub margin: Option<f64>,
    /// Count a future at its full value instead of a margin.
    #[serde(default)]
    pub margin_notional: Option<bool>,
}

const COLS: &str = "provider, asset_type, ticker, mintick, pointvalue, timezone, session, kind, currency, basecurrency, description, \
     margin, margin_notional";

pub async fn get(pool: &PgPool, provider: &str, asset_type: &str, ticker: &str) -> anyhow::Result<Option<InstrumentMeta>> {
    let sql = format!("SELECT {COLS} FROM instrument_meta WHERE provider = $1 AND asset_type = $2 AND ticker = $3");
    Ok(sqlx::query_as::<_, InstrumentMeta>(sqlx::AssertSqlSafe(sql))
        .bind(provider)
        .bind(asset_type)
        .bind(ticker)
        .fetch_optional(pool)
        .await?)
}

/// Insert or replace every field of one instrument.
pub async fn put(pool: &PgPool, m: &InstrumentMeta) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO instrument_meta (provider, asset_type, ticker, mintick, pointvalue, timezone, session, kind, \
         currency, basecurrency, description, margin, margin_notional) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
         ON CONFLICT (provider, asset_type, ticker) DO UPDATE SET mintick = EXCLUDED.mintick, \
         pointvalue = EXCLUDED.pointvalue, timezone = EXCLUDED.timezone, session = EXCLUDED.session, \
         kind = EXCLUDED.kind, currency = EXCLUDED.currency, basecurrency = EXCLUDED.basecurrency, \
         description = EXCLUDED.description, margin = EXCLUDED.margin, \
         margin_notional = EXCLUDED.margin_notional, updated_at = now()",
    )
    .bind(&m.provider)
    .bind(&m.asset_type)
    .bind(&m.ticker)
    .bind(m.mintick)
    .bind(m.pointvalue)
    .bind(&m.timezone)
    .bind(&m.session)
    .bind(&m.kind)
    .bind(&m.currency)
    .bind(&m.basecurrency)
    .bind(&m.description)
    .bind(m.margin)
    .bind(m.margin_notional)
    .execute(pool)
    .await
    .context("saving instrument metadata")?;
    Ok(())
}
