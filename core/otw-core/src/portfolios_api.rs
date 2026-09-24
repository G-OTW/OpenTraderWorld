//! HTTP API for the Portfolio Tracker module.
//!
//! Portfolios → assets (resolved provider symbols) → operations ledger. The listing endpoint
//! returns per-portfolio totals (market value, cost, PnL) in each portfolio's display currency;
//! the detail endpoint returns its assets (with derived position/PnL), the filterable operations
//! list, and the valuation snapshots for the chart. Symbol search resolves crypto via CoinGecko
//! and stocks/ETFs via Yahoo. Refresh re-prices (USD) and snapshots. Single-user: no owner scoping.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::portfolios::prices;
use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::portfolios as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/portfolios", get(list).post(create))
        .route("/api/portfolios/search", get(search))
        .route("/api/portfolios/{id}", get(detail).patch(update).delete(remove))
        .route("/api/portfolios/{id}/refresh", post(refresh))
        .route("/api/portfolios/{id}/reconcile", post(reconcile))
        .route("/api/portfolios/{id}/assets", post(add_asset))
        .route(
            "/api/portfolios/assets/{asset_id}",
            get(asset_detail).patch(patch_asset).delete(delete_asset),
        )
        .route("/api/portfolios/{id}/operations", post(add_portfolio_operation))
        .route("/api/portfolios/assets/{asset_id}/operations", post(add_operation))
        .route("/api/portfolios/operations/{op_id}", axum::routing::delete(delete_operation))
}

fn parse_date(s: Option<&str>) -> Result<Date, ApiError> {
    match s {
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
        None => Ok(time::OffsetDateTime::now_utc().date()),
    }
}

// ── Portfolios ────────────────────────────────────────────────────────────────

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let portfolios = store::list_portfolios(&state.pool).await?;
    let mut out = Vec::with_capacity(portfolios.len());
    for pf in &portfolios {
        out.push(store::summary(&state.pool, pf).await?);
    }
    Ok(Json(json!({ "portfolios": out })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateBody {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "usd")]
    currency: String,
}
fn usd() -> String {
    "USD".into()
}

/// What an asset can be bucketed as. The drift table reads these, so widening the list is
/// widening what a target allocation can talk about.
pub const ASSET_CLASSES: &[&str] =
    &["crypto", "stock", "etf", "bond", "commodity", "real_estate", "alternative"];

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let pf = store::create_portfolio(&state.pool, name, b.description.trim(), b.currency.trim()).await?;
    Ok(Json(json!({ "portfolio": pf })))
}

async fn detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let book = store::book(&state.pool, &pf).await?;
    let ops = store::list_portfolio_operations(&state.pool, id, &pf.currency).await?;
    let operations: Vec<Value> = ops
        .into_iter()
        .map(|(o, symbol, currency)| json!({ "operation": o, "symbol": symbol, "currency": currency }))
        .collect();
    let snapshots = store::list_snapshots(&state.pool, id).await?;
    let sparkline: Vec<f64> = snapshots.iter().rev().take(60).map(|s| s.market_value).rev().collect();
    // The book is one ledger walk; re-deriving a summary beside it would be a second.
    let summary = json!({
        "portfolio": pf,
        "asset_count": book.positions.len(),
        "market_value": book.market_value,
        "cost_basis": book.cost_basis,
        "unrealized": book.unrealized,
        "realized": book.realized,
        "cash_total": book.cash_total,
        "cash_tracked": book.cash_tracked,
        "net_worth": book.net_worth,
        "income": book.income.total,
        "sparkline": sparkline,
    });
    Ok(Json(json!({
        "portfolio": pf,
        "summary": summary,
        "book": book,
        "assets": book.positions,
        "operations": operations,
        "snapshots": snapshots
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateBody {
    name: Option<String>,
    description: Option<String>,
    /// The long note (investment thesis). Kept verbatim apart from trimming the edges.
    notes: Option<String>,
    currency: Option<String>,
    auto_refresh: Option<bool>,
    /// The instrument the portfolio is measured against: `{"asset_type":"equity","symbol":"SPY"}`.
    /// Absent leaves it; explicit `null` clears it, and the benchmark block goes away with it.
    #[serde(default, deserialize_with = "double_option_json")]
    benchmark: Option<Option<Value>>,
    /// Annual risk-free rate as a fraction (0.03 = 3%), used by Sharpe and Sortino.
    risk_free: Option<f64>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::update_portfolio(
        &state.pool,
        id,
        b.name.as_deref().map(str::trim),
        b.description.as_deref().map(str::trim),
        b.notes.as_deref().map(str::trim),
        b.currency.as_deref().map(str::trim),
        b.auto_refresh,
        b.benchmark.as_ref().map(|o| o.as_ref()),
        b.risk_free,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    Ok(Json(json!({ "portfolio": pf })))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    store::delete_portfolio(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Minimum spacing between two manual refreshes of the same portfolio. The upstream price
/// sources are free, keyless, per-IP rate-limited endpoints, and a throttled fetch fails
/// silently (the asset keeps its old price), so a hammered refresh looks successful while
/// returning stale data. The daily auto-refresh job calls `refresh_portfolio` directly and is
/// deliberately not subject to this.
const REFRESH_COOLDOWN: time::Duration = time::Duration::minutes(1);

/// Seconds the caller must wait before refreshing again, or `None` if a refresh is allowed.
/// A clock skew / future `last` timestamp must not lock the user out, so only a positive,
/// under-cooldown gap blocks.
fn cooldown_remaining(
    last: Option<time::OffsetDateTime>,
    now: time::OffsetDateTime,
) -> Option<i64> {
    let elapsed = now - last?;
    (elapsed >= time::Duration::ZERO && elapsed < REFRESH_COOLDOWN)
        .then(|| (REFRESH_COOLDOWN - elapsed).whole_seconds().max(1))
}

async fn refresh(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    if let Some(wait) = cooldown_remaining(pf.refreshed_at, time::OffsetDateTime::now_utc()) {
        return Err(ApiError::too_many(&format!(
            "prices were just refreshed — wait {wait}s before refreshing again"
        )));
    }
    prices::refresh_portfolio(&state.pool, &pf).await?;
    // The curve catches up behind the button, not inside it: a backfill is minutes of
    // downloads and the response is a price table the user is already looking at.
    crate::portfolios::history::spawn_catch_up(state.pool.clone(), pf.clone());
    let summary = store::summary(&state.pool, &pf).await?;
    Ok(Json(json!({ "summary": summary })))
}

/// Check every asset against its (possibly overridden) price source, persisting an ok/unresolved
/// status per asset. Returns the per-asset results so the client can drive the reconcile modal.
async fn reconcile(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let results = prices::reconcile_portfolio(&state.pool, &pf).await?;
    let unresolved = results.iter().filter(|r| r.status == "unresolved").count();
    Ok(Json(json!({ "results": results, "unresolved": unresolved })))
}

// ── Symbol search ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    /// "crypto" | "stock". "stock" also returns ETFs.
    kind: String,
}

async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let hits = match q.kind.as_str() {
        "crypto" => prices::search_crypto(&state.pool, &q.q).await?,
        "stock" | "etf" => prices::search_stock(&q.q).await?,
        _ => return Err(ApiError::bad_request("kind must be 'crypto' or 'stock'")),
    };
    Ok(Json(json!({ "results": hits })))
}

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAssetBody {
    asset_class: String,
    provider: String,
    provider_id: String,
    symbol: String,
    #[serde(default)]
    name: String,
    /// Currency the asset's operations are entered in. Defaults to the portfolio's.
    #[serde(default)]
    currency: Option<String>,
}

async fn add_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddAssetBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(b.provider.as_str(), "coingecko" | "yahoo") {
        return Err(ApiError::bad_request("unknown provider"));
    }
    if b.provider_id.trim().is_empty() {
        return Err(ApiError::bad_request("provider_id is required"));
    }
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    // The currency the asset's operations are entered in. Defaults to the portfolio's display
    // currency, which is what the user is looking at when they add it.
    let currency = b.currency.as_deref().map(str::trim).unwrap_or(&pf.currency);
    if !CURRENCIES.contains(&currency) {
        return Err(ApiError::bad_request("unknown currency"));
    }
    let asset = store::add_asset(
        &state.pool,
        id,
        b.asset_class.trim(),
        b.provider.trim(),
        b.provider_id.trim(),
        b.symbol.trim(),
        b.name.trim(),
        currency,
    )
    .await?;
    // Price it right away: `refreshed_at` is portfolio-wide, so an asset added after the last
    // refresh would otherwise show blank price/value until the next one — and the manual
    // refresh is now cooldown-gated, which made that look like a permanently broken row.
    // Best-effort: a source hiccup just leaves the price blank, as before.
    let asset = match prices::price_new_asset(&state.pool, &asset).await {
        Ok(Some(updated)) => updated,
        Ok(None) => asset,
        Err(e) => {
            tracing::warn!("initial price for {} failed: {e:#}", asset.symbol);
            asset
        }
    };
    Ok(Json(json!({ "asset": asset })))
}

async fn asset_detail(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let asset = store::get_asset(&state.pool, asset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("asset not found"))?;
    let operations = store::list_operations(&state.pool, asset_id).await?;
    Ok(Json(json!({ "asset": asset, "operations": operations })))
}

async fn delete_asset(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_asset(&state.pool, asset_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Deserialize a JSON field so an *absent* key → None (leave unchanged) but an explicit `null` →
/// Some(None) (clear it). Needed to tell "don't touch spot_provider" from "remove the override".
fn double_option<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(d)?))
}

/// The same, for a JSON-valued field (the benchmark coordinate).
fn double_option_json<'de, D>(d: D) -> Result<Option<Option<Value>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<Value>::deserialize(d)?))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PatchAssetBody {
    /// Absent = leave; null = clear override; "binance"|"kraken"|"coinbase"|"yahoo"|"coingecko" = set.
    #[serde(default, deserialize_with = "double_option")]
    spot_provider: Option<Option<String>>,
    /// Provider-specific ticker for the override (e.g. "BTCUSDT").
    spot_symbol: Option<String>,
    /// "ok" | "unresolved" | "manual". Typically "manual" (opt out) or "ok" (clear a manual flag).
    recon_status: Option<String>,
    /// Ticker this asset's daily bars are stored under, for the history rebuild and every
    /// measure that needs a series. Asked once, never guessed.
    hist_symbol: Option<String>,
    /// Currency those bars are quoted in (a Paris listing is EUR even when the spot is USD).
    hist_currency: Option<String>,
    /// crypto | stock | etf | bond | commodity | real_estate | alternative. What the drift
    /// table buckets on.
    asset_class: Option<String>,
}

/// Patch an asset's price-source override and/or reconcile status. Validates the provider and
/// status enums here (the DB also constrains them). Returns the updated asset.
async fn patch_asset(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
    Json(b): Json<PatchAssetBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(Some(p)) = &b.spot_provider {
        if !matches!(p.as_str(), "coingecko" | "yahoo" | "binance" | "kraken" | "coinbase") {
            return Err(ApiError::bad_request("unknown spot_provider"));
        }
    }
    if let Some(s) = &b.recon_status {
        if !matches!(s.as_str(), "ok" | "unresolved" | "manual") {
            return Err(ApiError::bad_request("invalid recon_status"));
        }
    }
    if let Some(c) = &b.hist_currency {
        if !CURRENCIES.contains(&c.trim()) {
            return Err(ApiError::bad_request("unknown currency"));
        }
    }
    if let Some(c) = &b.asset_class {
        if !ASSET_CLASSES.contains(&c.trim()) {
            return Err(ApiError::bad_request("unknown asset class"));
        }
    }
    let spot_provider = b.spot_provider.as_ref().map(|o| o.as_deref());
    let asset = store::update_asset(
        &state.pool,
        asset_id,
        spot_provider,
        b.spot_symbol.as_deref().map(str::trim),
        b.recon_status.as_deref(),
        b.hist_symbol.as_deref().map(str::trim),
        b.hist_currency.as_deref().map(str::trim),
        b.asset_class.as_deref().map(str::trim),
    )
    .await?
    .ok_or_else(|| ApiError::not_found("asset not found"))?;
    Ok(Json(json!({ "asset": asset })))
}

// ── Operations ────────────────────────────────────────────────────────────────

/// A ledger row. `side` is the kind (see `store::OP_KINDS`): `buy` and `sell` move units and
/// need a quantity and a unit price; every other kind moves cash only and carries its amount
/// in `amount` (or `price`, which is the same field seen from the trade form).
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddOpBody {
    /// buy | sell | deposit | withdraw | dividend | interest | coupon | fee | tax.
    side: String,
    /// The line this row belongs to. Required for a buy or a sell, optional for income
    /// (a dividend names the holding that paid it), absent for portfolio-level cash.
    #[serde(default)]
    asset_id: Option<Uuid>,
    op_date: Option<String>,
    #[serde(default)]
    quantity: f64,
    #[serde(default)]
    price: f64,
    /// Cash amount, for a kind that has no unit price. Wins over `price` when both are sent.
    #[serde(default)]
    amount: Option<f64>,
    #[serde(default)]
    fee: f64,
    #[serde(default)]
    note: String,
    /// Currency of the amounts. Defaults to the asset's, else the portfolio's.
    #[serde(default)]
    currency: Option<String>,
}

/// Validate a body and book it. `asset` is the row's asset when the route named one.
async fn book_operation(
    state: &AppState,
    pf: &store::Portfolio,
    asset: Option<store::Asset>,
    b: AddOpBody,
) -> Result<store::Operation, ApiError> {
    let side = b.side.trim();
    if !store::OP_KINDS.contains(&side) {
        return Err(ApiError::bad_request(
            "unknown operation kind (buy, sell, deposit, withdraw, dividend, interest, coupon, fee, tax)",
        ));
    }
    let asset_id = b.asset_id.or_else(|| asset.as_ref().map(|a| a.id));
    if store::is_trade(side) && asset_id.is_none() {
        return Err(ApiError::bad_request("a buy or a sell needs an asset"));
    }
    // The amount lives in `price` with a quantity of 1 for every cash kind, so one shape
    // covers the whole ledger and the walk never branches to read it.
    let (quantity, price) = if store::is_trade(side) {
        if b.quantity <= 0.0 {
            return Err(ApiError::bad_request("quantity must be positive"));
        }
        (b.quantity, b.price)
    } else {
        let amount = b.amount.unwrap_or(b.price);
        if amount <= 0.0 {
            return Err(ApiError::bad_request("amount must be positive"));
        }
        (1.0, amount)
    };
    if b.fee < 0.0 {
        return Err(ApiError::bad_request("fee cannot be negative"));
    }
    // An asset row inherits its asset's currency by leaving the column NULL, which is what
    // every pre-existing row means. A cash row has nothing to inherit from, so it is stamped.
    let currency = match b.currency.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => {
            if !CURRENCIES.contains(&c) {
                return Err(ApiError::bad_request("unknown currency"));
            }
            Some(c.to_string())
        }
        None if asset_id.is_none() => Some(pf.currency.clone()),
        None => None,
    };
    let date = parse_date(b.op_date.as_deref())?;

    // An asset added after the last portfolio refresh has no spot yet, so its row would show
    // a position with blank price/value until the next one. Booking a trade is the moment the
    // user expects a live price, so fill it in here when it's still missing. Best-effort.
    if let Some(a) = asset.as_ref().filter(|a| a.last_price_usd.is_none()) {
        if let Err(e) = prices::price_new_asset(&state.pool, a).await {
            tracing::warn!("initial price for {} failed: {e:#}", a.symbol);
        }
    }
    let op = store::add_operation(
        &state.pool,
        pf.id,
        asset_id,
        side,
        date,
        quantity,
        price,
        b.fee,
        b.note.trim(),
        currency.as_deref(),
    )
    .await?;
    // Every snapshot from this day forward now describes a book that never existed. The
    // watermark says so; rebuilding is the user's call, not a side effect of typing a trade.
    store::mark_rebuild_from(&state.pool, pf.id, date).await?;
    Ok(op)
}

/// Book a row against a named asset (the position row's inline form).
async fn add_operation(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
    Json(b): Json<AddOpBody>,
) -> Result<Json<Value>, ApiError> {
    let asset = store::get_asset(&state.pool, asset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("asset not found"))?;
    let pf = store::get_portfolio(&state.pool, asset.portfolio_id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let op = book_operation(&state, &pf, Some(asset), b).await?;
    Ok(Json(json!({ "operation": op })))
}

/// Book a row against the portfolio itself: a deposit, a withdrawal, a fee, a tax, or
/// income that names its asset in the body.
async fn add_portfolio_operation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddOpBody>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    // A body naming an asset must name one of *this* portfolio's: the row carries the
    // portfolio id, and a mismatch would file the operation in a book it does not belong to.
    let asset = match b.asset_id {
        Some(aid) => {
            let a = store::get_asset(&state.pool, aid)
                .await?
                .ok_or_else(|| ApiError::not_found("asset not found"))?;
            if a.portfolio_id != id {
                return Err(ApiError::bad_request("that asset belongs to another portfolio"));
            }
            Some(a)
        }
        None => None,
    };
    let op = book_operation(&state, &pf, asset, b).await?;
    Ok(Json(json!({ "operation": op })))
}

async fn delete_operation(
    State(state): State<AppState>,
    Path(op_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // Read it before it is gone: the curve is stale from the day the row was on, and after
    // the delete there is nothing left to ask.
    let stale = store::operation_stamp(&state.pool, op_id).await?;
    store::delete_operation(&state.pool, op_id).await?;
    if let Some((portfolio_id, date)) = stale {
        store::mark_rebuild_from(&state.pool, portfolio_id, date).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Duration, OffsetDateTime};

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }

    #[test]
    fn never_refreshed_is_allowed() {
        assert_eq!(cooldown_remaining(None, now()), None);
    }

    #[test]
    fn just_refreshed_blocks_for_the_full_minute() {
        assert_eq!(cooldown_remaining(Some(now()), now()), Some(60));
    }

    #[test]
    fn partway_through_reports_the_remainder() {
        let last = now() - Duration::seconds(45);
        assert_eq!(cooldown_remaining(Some(last), now()), Some(15));
    }

    /// The last second still blocks, and reports at least 1s rather than 0.
    #[test]
    fn final_second_still_blocks() {
        let last = now() - Duration::milliseconds(59_900);
        assert_eq!(cooldown_remaining(Some(last), now()), Some(1));
    }

    #[test]
    fn past_the_cooldown_is_allowed() {
        assert_eq!(cooldown_remaining(Some(now() - Duration::seconds(60)), now()), None);
        assert_eq!(cooldown_remaining(Some(now() - Duration::hours(3)), now()), None);
    }

    /// Clock skew (a timestamp in the future) must not lock the user out.
    #[test]
    fn future_timestamp_is_allowed() {
        assert_eq!(cooldown_remaining(Some(now() + Duration::hours(1)), now()), None);
    }
}
