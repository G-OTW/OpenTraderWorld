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
    /// One line, shown on the card next to the figures.
    pub description: String,
    /// The long note: investment thesis, purpose, rules. Free text.
    pub notes: String,
    pub currency: String,
    pub auto_refresh: bool,
    pub position: f64,
    /// What the portfolio is measured against: `{"asset_type":"equity","symbol":"SPY"}`.
    /// NULL = no benchmark, and the benchmark block is absent rather than zero-filled.
    pub benchmark: Option<serde_json::Value>,
    /// Annual risk-free rate used by Sharpe and Sortino, as a fraction (0.03 = 3%).
    pub risk_free: f64,
    /// Oldest day the stored curve is known to be wrong from, set by a backdated edit.
    #[serde(with = "date_opt")]
    pub rebuild_from: Option<Date>,
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
    /// Ticker this asset's daily bars are stored under. A CoinGecko coin id is a *spot*
    /// coordinate, not a bar ticker, so it is asked once and never guessed: empty means the
    /// asset is named in the coverage report rather than approximated to something priceable.
    pub hist_symbol: String,
    /// Currency those bars are quoted in. A spot is USD by construction; a bar is quoted in
    /// whatever the listing trades in.
    pub hist_currency: String,
}

impl Asset {
    /// The ticker this asset's daily bars are stored under, or empty when nobody can say.
    ///
    /// A Yahoo `provider_id` **is** an exchange ticker (`SXRL.DE`, `SPY`): it is the same
    /// string the downloader files bars under, so reading it is not a guess. A CoinGecko
    /// coin id is a spot coordinate with no tape, so it stays empty and the asset is named
    /// in the coverage report rather than approximated to something priceable.
    pub fn bar_symbol(&self) -> &str {
        let sym = self.hist_symbol.trim();
        if !sym.is_empty() {
            return sym;
        }
        if self.provider == "yahoo" { self.provider_id.trim() } else { "" }
    }
}

/// One ledger row. `side` is the kind: `buy` and `sell` move units, everything else moves
/// cash only (see [`OP_KINDS`]).
///
/// A cash row carries its amount in `price` with a `quantity` of 1, so `quantity × price` is
/// the gross amount of *every* row and the ledger walk needs no branch to read it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Operation {
    pub id: Uuid,
    /// NULL on a portfolio-level row (deposit, withdrawal, a fee or tax that names no line).
    pub asset_id: Option<Uuid>,
    pub side: String,
    #[serde(with = "date_fmt")]
    pub op_date: Date,
    pub quantity: f64,
    pub price: f64,
    pub fee: f64,
    pub note: String,
    /// Currency the amounts are written in. NULL on an asset row means the asset's own.
    pub currency: Option<String>,
}

/// Every ledger kind, in the order the form offers them.
pub const OP_KINDS: &[&str] = &[
    "buy", "sell", "deposit", "withdraw", "dividend", "interest", "coupon", "fee", "tax",
];

/// Kinds that move units and therefore require an asset.
pub fn is_trade(kind: &str) -> bool {
    matches!(kind, "buy" | "sell")
}

/// Kinds that pay the holder. Income is **not** realized capital gain: it is reported beside
/// it, because folding it in would make every per-position return wrong.
pub fn is_income(kind: &str) -> bool {
    matches!(kind, "dividend" | "interest" | "coupon")
}

/// Kinds that are a cost in their own right, as opposed to a fee riding on a trade.
pub fn is_cost(kind: &str) -> bool {
    matches!(kind, "fee" | "tax")
}

/// One day of the portfolio's valuation. `market_value` is positions only (that is what the
/// chart has always drawn); net worth is `market_value + cash`.
///
/// `flow` is the net external cash of the day, and it is the whole reason this row exists in
/// this shape: without it every deposit reads as a rally and no risk measure computed on the
/// series means anything.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    #[serde(with = "date_fmt")]
    pub snap_date: Date,
    pub currency: String,
    pub market_value: f64,
    pub cost_basis: f64,
    pub cash: f64,
    pub flow: f64,
    pub income: f64,
    pub fees: f64,
    /// `live` (priced by a refresh) or `rebuilt` (walked out of the ledger and stored bars).
    pub source: String,
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
    /// Uninvested cash, display currency.
    pub cash_total: f64,
    /// Whether the ledger tracks a cash account at all. See `Book::cash_tracked`.
    pub cash_tracked: bool,
    /// Positions plus cash: what the portfolio is actually worth.
    pub net_worth: f64,
    /// Dividends, interest and coupons collected over the whole ledger.
    pub income: f64,
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
mod date_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?),
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

const PF_COLS: &str = "id, name, description, notes, currency, auto_refresh, position, benchmark, risk_free, rebuild_from, refreshed_at, created_at, updated_at";
const ASSET_COLS: &str = "id, portfolio_id, asset_class, provider, provider_id, symbol, name, currency, last_price_usd, last_price_at, spot_provider, spot_symbol, recon_status, recon_checked_at, recon_note, hist_symbol, hist_currency";

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
#[allow(clippy::too_many_arguments)]
pub async fn update_portfolio(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    notes: Option<&str>,
    currency: Option<&str>,
    auto_refresh: Option<bool>,
    benchmark: Option<Option<&serde_json::Value>>,
    risk_free: Option<f64>,
) -> anyhow::Result<Option<Portfolio>> {
    // `benchmark` is tri-state: leave it, clear it, or set it. A flag says "touch this
    // column" so a clear ($8 = NULL) stays distinguishable from "do not touch".
    let touch_benchmark = benchmark.is_some();
    let benchmark_val = benchmark.flatten();
    let sql = format!(
        "UPDATE portfolios SET \
         name = COALESCE($2, name), \
         description = COALESCE($3, description), \
         notes = COALESCE($4, notes), \
         currency = COALESCE($5, currency), \
         auto_refresh = COALESCE($6, auto_refresh), \
         benchmark = CASE WHEN $7 THEN $8 ELSE benchmark END, \
         risk_free = COALESCE($9, risk_free), \
         updated_at = now() \
         WHERE id = $1 RETURNING {PF_COLS}"
    );
    Ok(sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(notes)
        .bind(currency)
        .bind(auto_refresh)
        .bind(touch_benchmark)
        .bind(benchmark_val)
        .bind(risk_free)
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
    hist_symbol: Option<&str>,
    hist_currency: Option<&str>,
    asset_class: Option<&str>,
) -> anyhow::Result<Option<Asset>> {
    // `spot_provider` is tri-state: None = leave, Some(None) = clear, Some(Some(p)) = set. We flag
    // "touch this column" separately so a clear ($2=NULL) is distinguishable from "leave alone".
    let touch_provider = spot_provider.is_some();
    let provider_val = spot_provider.flatten();
    let sql = format!(
        "UPDATE portfolio_assets SET \
         spot_provider = CASE WHEN $2 THEN $3 ELSE spot_provider END, \
         spot_symbol   = COALESCE($4, spot_symbol), \
         recon_status  = COALESCE($5, recon_status), \
         hist_symbol   = COALESCE($6, hist_symbol), \
         hist_currency = COALESCE($7, hist_currency), \
         asset_class   = COALESCE($8, asset_class) \
         WHERE id = $1 RETURNING {ASSET_COLS}"
    );
    Ok(sqlx::query_as::<_, Asset>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(touch_provider)
        .bind(provider_val)
        .bind(spot_symbol)
        .bind(recon_status)
        .bind(hist_symbol)
        .bind(hist_currency)
        .bind(asset_class)
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

const OP_COLS: &str = "id, asset_id, side, op_date, quantity, price, fee, note, currency";

pub async fn list_operations(pool: &PgPool, asset_id: Uuid) -> anyhow::Result<Vec<Operation>> {
    let sql = format!(
        "SELECT {OP_COLS} FROM portfolio_operations WHERE asset_id = $1 \
         ORDER BY op_date DESC, created_at DESC"
    );
    Ok(sqlx::query_as::<_, Operation>(sqlx::AssertSqlSafe(sql))
        .bind(asset_id)
        .fetch_all(pool)
        .await
        .context("listing operations")?)
}

/// Every operation of a portfolio, oldest first, for the single ledger walk.
pub async fn portfolio_operations_asc(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Operation>> {
    portfolio_operations(pool, portfolio_id).await
}

async fn portfolio_operations(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Operation>> {
    let sql = format!(
        "SELECT {OP_COLS} FROM portfolio_operations WHERE portfolio_id = $1 \
         ORDER BY op_date, created_at"
    );
    Ok(sqlx::query_as::<_, Operation>(sqlx::AssertSqlSafe(sql))
        .bind(portfolio_id)
        .fetch_all(pool)
        .await
        .context("listing portfolio operations")?)
}

/// Every operation in a portfolio with its asset's symbol (empty for a cash row) and the
/// currency the amounts are entered in, newest first, for the ledger table.
pub async fn list_portfolio_operations(
    pool: &PgPool,
    portfolio_id: Uuid,
    fallback_currency: &str,
) -> anyhow::Result<Vec<(Operation, String, String)>> {
    let rows = sqlx::query_as::<
        _,
        (Option<Uuid>, Uuid, String, Date, f64, f64, f64, String, Option<String>, Option<String>, Option<String>),
    >(
        "SELECT o.asset_id, o.id, o.side, o.op_date, o.quantity, o.price, o.fee, o.note, \
                o.currency, a.symbol, a.currency \
         FROM portfolio_operations o LEFT JOIN portfolio_assets a ON a.id = o.asset_id \
         WHERE o.portfolio_id = $1 ORDER BY o.op_date DESC, o.created_at DESC",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("listing portfolio operations")?;
    Ok(rows
        .into_iter()
        .map(
            |(asset_id, id, side, op_date, quantity, price, fee, note, currency, symbol, asset_ccy)| {
                let shown = currency
                    .clone()
                    .or(asset_ccy)
                    .unwrap_or_else(|| fallback_currency.to_string());
                (
                    Operation { id, asset_id, side, op_date, quantity, price, fee, note, currency },
                    symbol.unwrap_or_default(),
                    shown,
                )
            },
        )
        .collect())
}

/// Insert a ledger row. A cash kind stores its amount in `price` with a quantity of 1, which
/// is enforced here rather than trusted from the caller: the walk relies on it.
#[allow(clippy::too_many_arguments)]
pub async fn add_operation(
    pool: &PgPool,
    portfolio_id: Uuid,
    asset_id: Option<Uuid>,
    side: &str,
    op_date: Date,
    quantity: f64,
    price: f64,
    fee: f64,
    note: &str,
    currency: Option<&str>,
) -> anyhow::Result<Operation> {
    let quantity = if is_trade(side) { quantity } else { 1.0 };
    let sql = format!(
        "INSERT INTO portfolio_operations \
           (id, asset_id, portfolio_id, side, op_date, quantity, price, fee, note, currency) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING {OP_COLS}"
    );
    Ok(sqlx::query_as::<_, Operation>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(asset_id)
        .bind(portfolio_id)
        .bind(side)
        .bind(op_date)
        .bind(quantity)
        .bind(price)
        .bind(fee)
        .bind(note)
        .bind(currency)
        .fetch_one(pool)
        .await
        .context("adding operation")?)
}

/// Which portfolio a row belongs to and what day it sits on, for the staleness watermark.
pub async fn operation_stamp(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<(Uuid, Date)>> {
    Ok(sqlx::query_as::<_, (Option<Uuid>, Date)>(
        "SELECT portfolio_id, op_date FROM portfolio_operations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading an operation stamp")?
    .and_then(|(pf, d)| pf.map(|pf| (pf, d))))
}

pub async fn delete_operation(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM portfolio_operations WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting operation")?;
    Ok(())
}

// ── The book: one ledger walk per portfolio ───────────────────────────────────

/// Income the holdings paid. Gross: the fee that rode on an income row is a cost and is
/// reported as one.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Income {
    pub dividends: f64,
    pub interest: f64,
    pub coupons: f64,
    pub total: f64,
}

/// What the book cost to run. `transaction_fees` ride on a buy or a sell (and are already
/// inside cost basis and proceeds); `standalone_fees` and `taxes` are their own rows.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Costs {
    pub transaction_fees: f64,
    pub standalone_fees: f64,
    pub taxes: f64,
    pub total: f64,
}

/// Uninvested cash in one currency, in that currency. Negative is reported, never clamped:
/// it means margin, or a ledger that is missing its deposits, and both are worth seeing.
#[derive(Debug, Clone, Serialize)]
pub struct CashBalance {
    pub currency: String,
    pub amount: f64,
}

/// Everything derivable from the ledger alone, in the portfolio's display currency.
///
/// One walk, one FX pass. This is the `Book` substrate the analytics registry borrows: no
/// analyzer reads the ledger itself.
#[derive(Debug, Clone, Serialize)]
pub struct Book {
    pub currency: String,
    pub positions: Vec<AssetView>,
    /// Per currency, in that currency, so the user sees what is actually sitting there.
    pub cash: Vec<CashBalance>,
    /// The same cash converted to the display currency.
    pub cash_total: f64,
    /// Whether the ledger holds a cash account at all: at least one deposit or withdrawal.
    /// False means the user logs what was bought and not where the money came from, and
    /// `cash` is empty rather than the negative of everything ever spent.
    pub cash_tracked: bool,
    pub market_value: f64,
    pub cost_basis: f64,
    pub unrealized: f64,
    pub realized: f64,
    pub income: Income,
    pub costs: Costs,
    /// Positions marked to market plus cash. The headline figure.
    pub net_worth: f64,
    /// Assets holding units but carrying no price yet: `market_value` is short by that much.
    pub unpriced: i64,
}

/// Running position of one asset, in USD.
#[derive(Default)]
struct Leg {
    qty: f64,
    basis: f64,
    realized: f64,
}

/// Walk a portfolio's whole ledger once.
///
/// Every amount converts to USD at **its own operation's date**, so a buy made in 2019 keeps
/// the rate that applied then instead of being re-valued at today's. A missing rate falls
/// back to the raw amount, as everywhere else in the module.
pub async fn book(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<Book> {
    let today = OffsetDateTime::now_utc().date();
    let assets = list_assets(pool, pf.id).await?;
    let ops = portfolio_operations(pool, pf.id).await?;

    let asset_ccy: std::collections::HashMap<Uuid, String> =
        assets.iter().map(|a| (a.id, a.currency.clone())).collect();
    let mut legs: std::collections::HashMap<Uuid, Leg> = std::collections::HashMap::new();
    let mut cash_by_ccy: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    let mut cash_usd = 0.0_f64;
    let mut income = Income::default();
    let mut costs = Costs::default();
    let mut fx = journal_fx::FxCache::new();

    // A ledger holding no deposit and no withdrawal is not tracking a cash account: the user
    // logs what was bought, not where the money came from. Booking the trade legs into cash
    // anyway turns every purchase into an overdraft, so net worth collapses to the unrealized
    // PnL and every share of it runs to several hundred percent. No cash account, no cash.
    let cash_tracked = ops.iter().any(|o| matches!(o.side.as_str(), "deposit" | "withdraw"));

    for op in &ops {
        // Where the row's amounts are written: the row says so, else its asset, else the
        // portfolio. A cash row without a currency cannot exist (the DB refuses it).
        let ccy = op
            .currency
            .clone()
            .or_else(|| op.asset_id.and_then(|id| asset_ccy.get(&id).cloned()))
            .unwrap_or_else(|| pf.currency.clone());
        let gross = op.quantity * op.price;
        // Two sequential conversions rather than a closure: the FX cache is borrowed
        // mutably, and a closure holding it could not be called twice in one expression.
        let (gross_usd, fee_usd) = if ccy == "USD" {
            (gross, op.fee)
        } else {
            let g = fx.convert(pool, gross, &ccy, "USD", op.op_date).await?.unwrap_or(gross);
            let f = fx.convert(pool, op.fee, &ccy, "USD", op.op_date).await?.unwrap_or(op.fee);
            (g, f)
        };
        // Cash moves in the row's own currency for the per-currency view, and in USD for
        // every total: converting the total once at the end would re-value an old deposit.
        let mut cash = |native: f64, usd: f64| {
            if !cash_tracked {
                return;
            }
            *cash_by_ccy.entry(ccy.clone()).or_default() += native;
            cash_usd += usd;
        };

        match op.side.as_str() {
            "buy" => {
                let leg = legs.entry(op.asset_id.unwrap_or_default()).or_default();
                leg.qty += op.quantity;
                leg.basis += gross_usd + fee_usd;
                costs.transaction_fees += fee_usd;
                cash(-(gross + op.fee), -(gross_usd + fee_usd));
            }
            "sell" => {
                let leg = legs.entry(op.asset_id.unwrap_or_default()).or_default();
                let avg = if leg.qty > 0.0 { leg.basis / leg.qty } else { 0.0 };
                let sold = op.quantity.min(leg.qty.max(0.0));
                leg.realized += (gross_usd - fee_usd) - sold * avg;
                leg.qty -= op.quantity;
                leg.basis -= sold * avg;
                if leg.qty <= 0.0 {
                    leg.qty = 0.0;
                    leg.basis = 0.0;
                }
                costs.transaction_fees += fee_usd;
                cash(gross - op.fee, gross_usd - fee_usd);
            }
            "deposit" => cash(gross - op.fee, gross_usd - fee_usd),
            "withdraw" => cash(-(gross + op.fee), -(gross_usd + fee_usd)),
            k if is_income(k) => {
                match k {
                    "dividend" => income.dividends += gross_usd,
                    "interest" => income.interest += gross_usd,
                    _ => income.coupons += gross_usd,
                }
                costs.standalone_fees += fee_usd;
                cash(gross - op.fee, gross_usd - fee_usd);
            }
            "fee" => {
                costs.standalone_fees += gross_usd + fee_usd;
                cash(-(gross + op.fee), -(gross_usd + fee_usd));
            }
            "tax" => {
                costs.taxes += gross_usd + fee_usd;
                cash(-(gross + op.fee), -(gross_usd + fee_usd));
            }
            // The CHECK constraint is the gate; an unknown kind here is a migration ahead of
            // the binary, and dropping it silently is better than mis-booking it.
            _ => {}
        }
    }
    income.total = income.dividends + income.interest + income.coupons;
    costs.total = costs.transaction_fees + costs.standalone_fees + costs.taxes;

    // One conversion pass, today's rate, for everything that is a *current* figure.
    let conv = |usd: f64| async move { journal_fx::convert(pool, usd, "USD", &pf.currency, today).await };

    let mut positions = Vec::with_capacity(assets.len());
    let (mut market_value, mut cost_basis, mut unrealized, mut realized) = (0.0, 0.0, 0.0, 0.0);
    let mut unpriced = 0_i64;
    for a in assets {
        let leg = legs.remove(&a.id).unwrap_or_default();
        let basis = conv(leg.basis).await?.unwrap_or(leg.basis);
        let realized_pf = conv(leg.realized).await?.unwrap_or(leg.realized);
        let avg_cost = (leg.qty > 0.0).then(|| basis / leg.qty);
        let (price, mv, unreal) = match a.last_price_usd {
            Some(p) => {
                let price = conv(p).await?.unwrap_or(p);
                let mv = price * leg.qty;
                (Some(price), Some(mv), Some(mv - basis))
            }
            None => (None, None, None),
        };
        if price.is_none() && leg.qty > 0.0 {
            unpriced += 1;
        }
        market_value += mv.unwrap_or(0.0);
        cost_basis += basis;
        unrealized += unreal.unwrap_or(0.0);
        realized += realized_pf;
        positions.push(AssetView {
            asset: a,
            quantity: leg.qty,
            avg_cost,
            cost_basis: basis,
            price,
            market_value: mv,
            unrealized: unreal,
            realized: realized_pf,
        });
    }
    positions.sort_by(|a, b| a.asset.symbol.cmp(&b.asset.symbol));

    let cash_total = conv(cash_usd).await?.unwrap_or(cash_usd);
    let to_pf = |usd: f64| async move { journal_fx::convert(pool, usd, "USD", &pf.currency, today).await };
    let income = Income {
        dividends: to_pf(income.dividends).await?.unwrap_or(income.dividends),
        interest: to_pf(income.interest).await?.unwrap_or(income.interest),
        coupons: to_pf(income.coupons).await?.unwrap_or(income.coupons),
        total: to_pf(income.total).await?.unwrap_or(income.total),
    };
    let costs = Costs {
        transaction_fees: to_pf(costs.transaction_fees).await?.unwrap_or(costs.transaction_fees),
        standalone_fees: to_pf(costs.standalone_fees).await?.unwrap_or(costs.standalone_fees),
        taxes: to_pf(costs.taxes).await?.unwrap_or(costs.taxes),
        total: to_pf(costs.total).await?.unwrap_or(costs.total),
    };

    Ok(Book {
        currency: pf.currency.clone(),
        positions,
        cash: cash_by_ccy
            .into_iter()
            .filter(|(_, amount)| amount.abs() > 1e-9)
            .map(|(currency, amount)| CashBalance { currency, amount })
            .collect(),
        cash_total,
        cash_tracked,
        market_value,
        cost_basis,
        unrealized,
        realized,
        income,
        costs,
        net_worth: market_value + cash_total,
        unpriced,
    })
}

/// Computed views for every asset in a portfolio (in the portfolio's currency, today's FX).
pub async fn asset_views(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<Vec<AssetView>> {
    Ok(book(pool, pf).await?.positions)
}

/// One-row totals for the listing page.
pub async fn summary(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<PortfolioSummary> {
    let b = book(pool, pf).await?;
    let sparkline = sparkline_values(pool, pf.id).await?;
    Ok(PortfolioSummary {
        asset_count: b.positions.len() as i64,
        market_value: b.market_value,
        cost_basis: b.cost_basis,
        unrealized: b.unrealized,
        realized: b.realized,
        cash_total: b.cash_total,
        cash_tracked: b.cash_tracked,
        net_worth: b.net_worth,
        income: b.income.total,
        portfolio: pf.clone(),
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

const SNAP_COLS: &str =
    "snap_date, currency, market_value, cost_basis, cash, flow, income, fees, source";

pub async fn list_snapshots(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAP_COLS} FROM portfolio_snapshots WHERE portfolio_id = $1 ORDER BY snap_date"
    );
    Ok(sqlx::query_as::<_, Snapshot>(sqlx::AssertSqlSafe(sql))
        .bind(portfolio_id)
        .fetch_all(pool)
        .await
        .context("listing snapshots")?)
}

/// The stored curve over a window, oldest first. `from`/`to` are inclusive.
///
/// One indexed range scan: a five-year analysis must not replay ten thousand operations,
/// which is the whole reason the curve is materialized rather than folded per request.
pub async fn snapshots_between(
    pool: &PgPool,
    portfolio_id: Uuid,
    from: Option<Date>,
    to: Option<Date>,
) -> anyhow::Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAP_COLS} FROM portfolio_snapshots \
         WHERE portfolio_id = $1 \
           AND ($2::date IS NULL OR snap_date >= $2) \
           AND ($3::date IS NULL OR snap_date <= $3) \
         ORDER BY snap_date"
    );
    Ok(sqlx::query_as::<_, Snapshot>(sqlx::AssertSqlSafe(sql))
        .bind(portfolio_id)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
        .context("reading the curve")?)
}

/// The first and last day the curve covers, or `None` when there is no curve at all.
pub async fn snapshot_span(
    pool: &PgPool,
    portfolio_id: Uuid,
) -> anyhow::Result<Option<(Date, Date)>> {
    let row: (Option<Date>, Option<Date>) = sqlx::query_as(
        "SELECT MIN(snap_date), MAX(snap_date) FROM portfolio_snapshots WHERE portfolio_id = $1",
    )
    .bind(portfolio_id)
    .fetch_one(pool)
    .await
    .context("reading the curve span")?;
    Ok(match row {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    })
}

/// Upsert one day of the curve. `source` says where the numbers came from, and a rebuild
/// never overwrites a live snapshot: a price fetched on the day is better evidence than a
/// close read back out of a daily bar months later.
#[allow(clippy::too_many_arguments)]
pub async fn write_snapshot(
    pool: &PgPool,
    portfolio_id: Uuid,
    date: Date,
    currency: &str,
    market_value: f64,
    cost_basis: f64,
    cash: f64,
    flow: f64,
    income: f64,
    fees: f64,
    market_value_usd: Option<f64>,
    source: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO portfolio_snapshots \
           (portfolio_id, snap_date, currency, market_value, cost_basis, cash, flow, income, fees, \
            market_value_usd, source) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT (portfolio_id, snap_date) DO UPDATE SET \
           currency = EXCLUDED.currency, market_value = EXCLUDED.market_value, \
           cost_basis = EXCLUDED.cost_basis, cash = EXCLUDED.cash, flow = EXCLUDED.flow, \
           income = EXCLUDED.income, fees = EXCLUDED.fees, \
           market_value_usd = EXCLUDED.market_value_usd, source = EXCLUDED.source \
         WHERE portfolio_snapshots.source = EXCLUDED.source OR EXCLUDED.source = 'live'",
    )
    .bind(portfolio_id)
    .bind(date)
    .bind(currency)
    .bind(market_value)
    .bind(cost_basis)
    .bind(cash)
    .bind(flow)
    .bind(income)
    .bind(fees)
    .bind(market_value_usd)
    .bind(source)
    .execute(pool)
    .await
    .context("writing snapshot")?;
    Ok(())
}

/// Mark the curve wrong from `date` onward. A backdated operation invalidates every snapshot
/// after it, and rebuilding the whole history each time would re-download years of bars to
/// fix one week. The watermark only ever moves backwards.
pub async fn mark_rebuild_from(pool: &PgPool, portfolio_id: Uuid, date: Date) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE portfolios SET rebuild_from = LEAST(COALESCE(rebuild_from, $2), $2) WHERE id = $1",
    )
    .bind(portfolio_id)
    .bind(date)
    .execute(pool)
    .await
    .context("marking the curve stale")?;
    Ok(())
}

pub async fn clear_rebuild_from(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE portfolios SET rebuild_from = NULL WHERE id = $1")
        .bind(portfolio_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Upsert today's snapshot from the portfolio's current book.
///
/// `flow`, `income` and `fees` are the day's own movements, read from the ledger rather than
/// from the running totals: a snapshot is a day, and the difference between two cumulative
/// figures is not one when a day is missing.
///
/// **A partial book is not written.** An asset that has never been priced contributes zero to
/// `market_value`, so storing that day would record a portfolio short by that asset's entire
/// worth, and a stored day is permanent: it becomes a crater in the curve and it owns the
/// max drawdown from then on. A missing day is recoverable, a wrong one is not, so the write
/// is skipped and the next refresh takes the snapshot.
pub async fn snapshot_today(pool: &PgPool, pf: &Portfolio) -> anyhow::Result<()> {
    let today = OffsetDateTime::now_utc().date();
    let b = book(pool, pf).await?;
    if b.unpriced > 0 {
        return Ok(());
    }
    let d = day_movements(pool, pf, today).await?;
    write_snapshot(
        pool,
        pf.id,
        today,
        &pf.currency,
        b.market_value,
        b.cost_basis,
        b.cash_total,
        d.flow,
        d.income,
        d.fees,
        None,
        "live",
    )
    .await
}

/// External cash, income and costs booked on one day, in the portfolio's currency.
#[derive(Debug, Clone, Copy, Default)]
pub struct DayMovements {
    /// Deposits minus withdrawals. Not a return, which is exactly why it is subtracted out.
    pub flow: f64,
    pub income: f64,
    pub fees: f64,
}

pub async fn day_movements(
    pool: &PgPool,
    pf: &Portfolio,
    date: Date,
) -> anyhow::Result<DayMovements> {
    let rows: Vec<(String, f64, f64, f64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT o.side, o.quantity, o.price, o.fee, o.currency, a.currency \
         FROM portfolio_operations o LEFT JOIN portfolio_assets a ON a.id = o.asset_id \
         WHERE o.portfolio_id = $1 AND o.op_date = $2",
    )
    .bind(pf.id)
    .bind(date)
    .fetch_all(pool)
    .await
    .context("reading the day's movements")?;

    let mut fx = journal_fx::FxCache::new();
    let mut out = DayMovements::default();
    for (side, quantity, price, fee, op_ccy, asset_ccy) in rows {
        let ccy = op_ccy.or(asset_ccy).unwrap_or_else(|| pf.currency.clone());
        let gross = quantity * price;
        let (gross_pf, fee_pf) = if ccy == pf.currency {
            (gross, fee)
        } else {
            (
                fx.convert(pool, gross, &ccy, &pf.currency, date).await?.unwrap_or(gross),
                fx.convert(pool, fee, &ccy, &pf.currency, date).await?.unwrap_or(fee),
            )
        };
        match side.as_str() {
            "deposit" => out.flow += gross_pf - fee_pf,
            "withdraw" => out.flow -= gross_pf + fee_pf,
            k if is_income(k) => out.income += gross_pf,
            "fee" => out.fees += gross_pf + fee_pf,
            "tax" => out.fees += gross_pf + fee_pf,
            "buy" | "sell" => out.fees += fee_pf,
            _ => {}
        }
    }
    Ok(out)
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
