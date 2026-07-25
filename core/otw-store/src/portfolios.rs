//! Storage for the Portfolio Tracker module.
//!
//! Portfolios hold assets; each asset is an operations ledger (buy/sell) from which current
//! quantity, average cost and PnL derive. Prices are stored in USD (`last_price_usd`); values are
//! converted to the portfolio's display currency at read time via the journal's USD-based fx_rates
//! (carry-forward). Daily valuation snapshots build a value time series. Single-user: no owner
//! scoping. Distinct from `mportfolios` (the Dataroma cache).

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::journal_fx;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Portfolio {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub currency: String,
    pub auto_refresh: bool,
    pub position: f64,
    #[serde(with = "ts_opt")]
    pub refreshed_at: Option<OffsetDateTime>,
    #[serde(with = "ts")]
    pub created_at: OffsetDateTime,
    #[serde(with = "ts")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Asset {
    pub id: Uuid,
    pub portfolio_id: Uuid,
    pub asset_class: String,
    pub provider: String,
    pub provider_id: String,
    pub symbol: String,
    pub name: String,
    /// Currency the asset's operations (price, fee) are entered in. Spot prices stay USD.
    pub currency: String,
    pub last_price_usd: Option<f64>,
    #[serde(with = "ts_opt")]
    pub last_price_at: Option<OffsetDateTime>,
    /// Overriding live-price source (NULL → price via `provider`/`provider_id`).
    pub spot_provider: Option<String>,
    /// Provider-specific ticker for `spot_provider` (e.g. Binance "BTCUSDT"), user-entered.
    pub spot_symbol: String,
    /// Last reconcile outcome: 'ok' | 'unresolved' | 'manual'. Only 'ok' is priced by refresh.
    pub recon_status: String,
    #[serde(with = "ts_opt")]
    pub recon_checked_at: Option<OffsetDateTime>,
    pub recon_note: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Operation {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub side: String,
    #[serde(with = "date_fmt")]
    pub op_date: Date,
    pub quantity: f64,
    pub price: f64,
    pub fee: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    #[serde(with = "date_fmt")]
    pub snap_date: Date,
    pub currency: String,
    pub market_value: f64,
    pub cost_basis: f64,
}

// ── Derived (computed) views returned to the API ──────────────────────────────

/// An asset with its ledger-derived position and PnL, all amounts in the portfolio currency.
#[derive(Debug, Clone, Serialize)]
pub struct AssetView {
    #[serde(flatten)]
    pub asset: Asset,
    /// Current units held (Σ buy − Σ sell).
    pub quantity: f64,
    /// Average cost per unit of the open position, in portfolio currency (incl. buy fees).
    pub avg_cost: Option<f64>,
    /// Cost basis of the open position, in portfolio currency.
    pub cost_basis: f64,
    /// Latest price converted to portfolio currency, or None if no price yet.
    pub price: Option<f64>,
    /// Market value of the open position, in portfolio currency.
    pub market_value: Option<f64>,
    /// Unrealized PnL (market_value − cost_basis), portfolio currency.
    pub unrealized: Option<f64>,
    /// Realized PnL from closed quantity, portfolio currency.
    pub realized: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioSummary {
    #[serde(flatten)]
    pub portfolio: Portfolio,
    pub asset_count: i64,
    pub market_value: f64,
    pub cost_basis: f64,
    pub unrealized: f64,
    pub realized: f64,
    /// Recent snapshot market values (oldest→newest) for the card sparkline.
    pub sparkline: Vec<f64>,
}

// ── Serde helpers ─────────────────────────────────────────────────────────────

mod ts {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}
mod ts_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}
mod date_fmt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

const PF_COLS: &str = "id, name, description, currency, auto_refresh, position, refreshed_at, created_at, updated_at";
const ASSET_COLS: &str = "id, portfolio_id, asset_class, provider, provider_id, symbol, name, currency, last_price_usd, last_price_at, spot_provider, spot_symbol, recon_status, recon_checked_at, recon_note";

// ── Portfolios CRUD ───────────────────────────────────────────────────────────

pub async fn list_portfolios(pool: &PgPool) -> anyhow::Result<Vec<Portfolio>> {
    let sql = format!("SELECT {PF_COLS} FROM portfolios ORDER BY position, created_at");
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing portfolios")?)
}

pub async fn get_portfolio(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Portfolio>> {
    let sql = format!("SELECT {PF_COLS} FROM portfolios WHERE id = $1");
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching portfolio")?)
}

pub async fn create_portfolio(
    pool: &PgPool,
    name: &str,
    description: &str,
    currency: &str,
) -> anyhow::Result<Portfolio> {
    let sql = format!(
        "INSERT INTO portfolios (id, name, description, currency, position) \
         VALUES ($1,$2,$3,$4, (SELECT COALESCE(MAX(position),0)+1 FROM portfolios)) \
         RETURNING {PF_COLS}"
    );
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(description)
        .bind(currency)
        .fetch_one(pool)
        .await
        .context("creating portfolio")?)
}

/// Patch mutable portfolio fields. Any `None` is left unchanged.
pub async fn update_portfolio(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    currency: Option<&str>,
    auto_refresh: Option<bool>,
) -> anyhow::Result<Option<Portfolio>> {
    let sql = format!(
        "UPDATE portfolios SET \
         name = COALESCE($2, name), \
         description = COALESCE($3, description), \
         currency = COALESCE($4, currency), \
         auto_refresh = COALESCE($5, auto_refresh), \
         updated_at = now() \
         WHERE id = $1 RETURNING {PF_COLS}"
    );
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(currency)
        .bind(auto_refresh)
        .fetch_optional(pool)
        .await
        .context("updating portfolio")?)
}

pub async fn delete_portfolio(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM portfolios WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting portfolio")?;
    Ok(())
}

/// Portfolios with auto_refresh on (used by the daily job).
pub async fn list_auto_refresh(pool: &PgPool) -> anyhow::Result<Vec<Portfolio>> {
    let sql = format!("SELECT {PF_COLS} FROM portfolios WHERE auto_refresh ORDER BY position");
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing auto-refresh portfolios")?)
}

// ── Assets ────────────────────────────────────────────────────────────────────

pub async fn list_assets(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Asset>> {
    let sql = format!("SELECT {ASSET_COLS} FROM portfolio_assets WHERE portfolio_id = $1 ORDER BY symbol");
    Ok(sqlx::query_as::<_, Asset>(sqlx::AssertSqlSafe(sql))
        .bind(portfolio_id)
        .fetch_all(pool)
        .await
        .context("listing assets")?)
}

pub async fn get_asset(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Asset>> {
    let sql = format!("SELECT {ASSET_COLS} FROM portfolio_assets WHERE id = $1");
    Ok(sqlx::query_as::<_, Asset>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching asset")?)
}

#[allow(clippy::too_many_arguments)]
pub async fn add_asset(
    pool: &PgPool,
    portfolio_id: Uuid,
    asset_class: &str,
    provider: &str,
    provider_id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
) -> anyhow::Result<Asset> {
    let sql = format!(
        "INSERT INTO portfolio_assets (id, portfolio_id, asset_class, provider, provider_id, symbol, name, currency) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         ON CONFLICT (portfolio_id, provider, provider_id) DO UPDATE SET symbol = EXCLUDED.symbol, name = EXCLUDED.name, currency = EXCLUDED.currency \
         RETURNING {ASSET_COLS}"
    );
    Ok(sqlx::query_as::<_, Asset>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(portfolio_id)
        .bind(asset_class)
        .bind(provider)
        .bind(provider_id)
        .bind(symbol)
        .bind(name)
        .bind(currency)
        .fetch_one(pool)
        .await
        .context("adding asset")?)
}

pub async fn delete_asset(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM portfolio_assets WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting asset")?;
    Ok(())
}

/// Store a freshly fetched USD spot for an asset.
pub async fn set_asset_price(pool: &PgPool, id: Uuid, price_usd: f64) -> anyhow::Result<()> {
    sqlx::query("UPDATE portfolio_assets SET last_price_usd = $2, last_price_at = now() WHERE id = $1")
        .bind(id)
        .bind(price_usd)
        .execute(pool)
        .await
        .context("setting asset price")?;
    Ok(())
}

/// Patch an asset's price-source override and/or reconcile status. Any `None` is left unchanged.
/// Passing `Some(None)` for `spot_provider` clears the override (back to the default provider).
pub async fn update_asset(
    pool: &PgPool,
    id: Uuid,
    spot_provider: Option<Option<&str>>,
    spot_symbol: Option<&str>,
    recon_status: Option<&str>,
) -> anyhow::Result<Option<Asset>> {
    // `spot_provider` is tri-state: None = leave, Some(None) = clear, Some(Some(p)) = set. We flag
    // "touch this column" separately so a clear ($2=NULL) is distinguishable from "leave alone".
    let touch_provider = spot_provider.is_some();
    let provider_val = spot_provider.flatten();
    let sql = format!(
        "UPDATE portfolio_assets SET \
         spot_provider = CASE WHEN $2 THEN $3 ELSE spot_provider END, \
         spot_symbol   = COALESCE($4, spot_symbol), \
         recon_status  = COALESCE($5, recon_status) \
         WHERE id = $1 RETURNING {ASSET_COLS}"
    );
    Ok(sqlx::query_as::<_, Asset>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(touch_provider)
        .bind(provider_val)
        .bind(spot_symbol)
        .bind(recon_status)
        .fetch_optional(pool)
        .await
        .context("updating asset")?)
}

/// Record the outcome of a reconcile check for an asset.
pub async fn set_recon(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    note: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE portfolio_assets SET recon_status = $2, recon_note = $3, recon_checked_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(note)
    .execute(pool)
    .await
    .context("setting recon status")?;
    Ok(())
}

// ── Operations ────────────────────────────────────────────────────────────────

pub async fn list_operations(pool: &PgPool, asset_id: Uuid) -> anyhow::Result<Vec<Operation>> {
    Ok(sqlx::query_as::<_, Operation>(
        "SELECT id, asset_id, side, op_date, quantity, price, fee, note \
         FROM portfolio_operations WHERE asset_id = $1 ORDER BY op_date DESC, created_at DESC",
    )
    .bind(asset_id)
    .fetch_all(pool)
    .await
    .context("listing operations")?)
}

/// Every operation in a portfolio (joined across its assets), for the filterable operations list.
/// Every operation in a portfolio, with its asset's symbol and the currency the amounts are
/// entered in (op price/fee are stored raw in the asset's currency, not converted).
pub async fn list_portfolio_operations(
    pool: &PgPool,
    portfolio_id: Uuid,
) -> anyhow::Result<Vec<(Operation, String, String)>> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, Date, f64, f64, f64, String, String, String)>(
        "SELECT o.id, o.asset_id, o.side, o.op_date, o.quantity, o.price, o.fee, o.note, a.symbol, a.currency \
         FROM portfolio_operations o JOIN portfolio_assets a ON a.id = o.asset_id \
         WHERE a.portfolio_id = $1 ORDER BY o.op_date DESC, o.created_at DESC",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("listing portfolio operations")?;
    Ok(rows
        .into_iter()
        .map(|(id, asset_id, side, op_date, quantity, price, fee, note, symbol, currency)| {
            (Operation { id, asset_id, side, op_date, quantity, price, fee, note }, symbol, currency)
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn add_operation(
    pool: &PgPool,
    asset_id: Uuid,
    side: &str,
    op_date: Date,
    quantity: f64,
    price: f64,
    fee: f64,
    note: &str,
) -> anyhow::Result<Operation> {
    Ok(sqlx::query_as::<_, Operation>(
        "INSERT INTO portfolio_operations (id, asset_id, side, op_date, quantity, price, fee, note) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         RETURNING id, asset_id, side, op_date, quantity, price, fee, note",
    )
    .bind(Uuid::new_v4())
    .bind(asset_id)
    .bind(side)
    .bind(op_date)
    .bind(quantity)
    .bind(price)
    .bind(fee)
    .bind(note)
    .fetch_one(pool)
    .await
    .context("adding operation")?)
}

pub async fn delete_operation(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM portfolio_operations WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting operation")?;
    Ok(())
}

// ── Derived position & PnL (weighted-average cost, USD ledger) ─────────────────

/// Walk an asset's operations (oldest first) accumulating an average-cost position, in USD.
/// Returns (open_qty, open_cost_basis_usd, realized_pnl_usd). Buy fee is added to basis;
/// sell fee reduces proceeds.
///
/// Operations are entered in the asset's `currency`; each one converts to USD at **its own**
/// date, so a historical buy keeps the rate that applied then instead of being re-valued at
/// today's. A missing rate falls back to the raw amount (same as elsewhere in the module).
async fn position_usd(
    pool: &PgPool,
    ops: &[Operation],
    currency: &str,
) -> anyhow::Result<(f64, f64, f64)> {
    let mut chrono: Vec<&Operation> = ops.iter().collect();
    chrono.sort_by(|a, b| a.op_date.cmp(&b.op_date));
    let (mut qty, mut basis, mut realized) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut fx = journal_fx::FxCache::new();
    for op in chrono {
        // Convert the per-unit price and the fee at the operation's date.
        let (price, fee) = if currency == "USD" {
            (op.price, op.fee)
        } else {
            (
                fx.convert(pool, op.price, currency, "USD", op.op_date).await?.unwrap_or(op.price),
                fx.convert(pool, op.fee, currency, "USD", op.op_date).await?.unwrap_or(op.fee),
            )
        };
        if op.side == "buy" {
            qty += op.quantity;
            basis += op.quantity * price + fee;
        } else {
            // Sell: realize against current average cost.
            let avg = if qty > 0.0 { basis / qty } else { 0.0 };
            let sold = op.quantity.min(qty.max(0.0));
            let proceeds = op.quantity * price - fee;
            realized += proceeds - sold * avg;
            qty -= op.quantity;
            basis -= sold * avg;
            if qty <= 0.0 {
                qty = 0.0;
                basis = 0.0;
            }
        }
    }
    Ok((qty, basis.max(0.0), realized))
}

/// Build the computed view for one asset, converting USD figures to `currency` on `date`.
async fn asset_view(
    pool: &PgPool,
    asset: Asset,
    ops: &[Operation],
    currency: &str,
    date: Date,
) -> anyhow::Result<AssetView> {
    let (qty, basis_usd, realized_usd) = position_usd(pool, ops, &asset.currency).await?;
    let conv = |usd: f64| async move { journal_fx::convert(pool, usd, "USD", currency, date).await };

    let cost_basis = conv(basis_usd).await?.unwrap_or(basis_usd);
    let realized = conv(realized_usd).await?.unwrap_or(realized_usd);
    let avg_cost = if qty > 0.0 { Some(cost_basis / qty) } else { None };

    let (price, market_value, unrealized) = match asset.last_price_usd {
        Some(p) => {
            let price = conv(p).await?.unwrap_or(p);
            let mv = price * qty;
            (Some(price), Some(mv), Some(mv - cost_basis))
        }
        None => (None, None, None),
    };

    Ok(AssetView {
        asset,
        quantity: qty,
        avg_cost,
        cost_basis,
        price,
        market_value,
        unrealized,
        realized,
    })
}

/// Computed views for every asset in a portfolio (in the portfolio's currency, today's FX).
pub async fn asset_views(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<Vec<AssetView>> {
    let today = OffsetDateTime::now_utc().date();
    let assets = list_assets(pool, pf.id).await?;
    let mut out = Vec::with_capacity(assets.len());
    for a in assets {
        let ops = list_operations(pool, a.id).await?;
        out.push(asset_view(pool, a, &ops, &pf.currency, today).await?);
    }
    Ok(out)
}

/// One-row totals for the listing page.
pub async fn summary(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<PortfolioSummary> {
    let views = asset_views(pool, pf).await?;
    let mut market_value = 0.0;
    let mut cost_basis = 0.0;
    let mut unrealized = 0.0;
    let mut realized = 0.0;
    for v in &views {
        market_value += v.market_value.unwrap_or(0.0);
        cost_basis += v.cost_basis;
        unrealized += v.unrealized.unwrap_or(0.0);
        realized += v.realized;
    }
    let sparkline = sparkline_values(pool, pf.id).await?;
    Ok(PortfolioSummary {
        portfolio: pf.clone(),
        asset_count: views.len() as i64,
        market_value,
        cost_basis,
        unrealized,
        realized,
        sparkline,
    })
}

/// Recent snapshot market values (oldest→newest), capped for the card sparkline.
async fn sparkline_values(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<f64>> {
    let mut rows: Vec<f64> = sqlx::query_scalar::<_, f64>(
        "SELECT market_value FROM portfolio_snapshots \
         WHERE portfolio_id = $1 ORDER BY snap_date DESC LIMIT 60",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("loading sparkline")?;
    rows.reverse();
    Ok(rows)
}

// ── Snapshots ─────────────────────────────────────────────────────────────────

pub async fn list_snapshots(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Snapshot>> {
    Ok(sqlx::query_as::<_, Snapshot>(
        "SELECT snap_date, currency, market_value, cost_basis FROM portfolio_snapshots \
         WHERE portfolio_id = $1 ORDER BY snap_date",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("listing snapshots")?)
}

/// Upsert today's snapshot for a portfolio from its current computed totals.
pub async fn snapshot_today(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<()> {
    let s = summary(pool, pf).await?;
    sqlx::query(
        "INSERT INTO portfolio_snapshots (portfolio_id, snap_date, currency, market_value, cost_basis) \
         VALUES ($1, CURRENT_DATE, $2, $3, $4) \
         ON CONFLICT (portfolio_id, snap_date) DO UPDATE SET \
         currency = EXCLUDED.currency, market_value = EXCLUDED.market_value, cost_basis = EXCLUDED.cost_basis",
    )
    .bind(pf.id)
    .bind(&pf.currency)
    .bind(s.market_value)
    .bind(s.cost_basis)
    .execute(pool)
    .await
    .context("writing snapshot")?;
    Ok(())
}

pub async fn mark_refreshed(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE portfolios SET refreshed_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── CoinGecko coin list cache ──────────────────────────────────────────────────

/// Cached coin list (raw [{id,symbol,name}]) and when it was fetched, or None if never.
pub async fn coingecko_cache(pool: &PgPool) -> anyhow::Result<Option<(serde_json::Value, OffsetDateTime)>> {
    let row: Option<(serde_json::Value, OffsetDateTime)> =
        sqlx::query_as("SELECT coins, fetched_at FROM portfolio_coingecko_cache WHERE id")
            .fetch_optional(pool)
            .await
            .context("reading coingecko cache")?;
    Ok(row)
}

pub async fn set_coingecko_cache(pool: &PgPool, coins: &serde_json::Value) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO portfolio_coingecko_cache (id, coins, fetched_at) VALUES (TRUE, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET coins = EXCLUDED.coins, fetched_at = now()",
    )
    .bind(coins)
    .execute(pool)
    .await
    .context("writing coingecko cache")?;
    Ok(())
}
