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

use crate::portfolios::{alerts, income, lots, overview, prices};
use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::portfolios as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/portfolios", get(list).post(create))
        .route("/api/portfolios/search", get(search))
        .route("/api/portfolios/overview", get(overview))
        .route("/api/portfolios/reviews/due", get(reviews_due))
        .route("/api/portfolios/{id}/income/calendar", get(income_calendar))
        .route("/api/portfolios/{id}/assets/{asset_id}/lots", get(asset_lots))
        .route("/api/portfolios/{id}/assets/{asset_id}/transfer", post(transfer))
        .route("/api/portfolios/{id}/plans", get(plans_list).post(plan_add))
        .route("/api/portfolios/{id}/plans/{plan_id}", axum::routing::delete(plan_delete))
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
    let today = time::OffsetDateTime::now_utc().date();
    match s {
        Some(s) => {
            let d = Date::parse(s, &Iso8601::DATE)
                .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)"))?;
            // A ledger records what happened. One day of slack: the user's today may already be
            // tomorrow in UTC.
            if d > today.next_day().unwrap_or(today) {
                return Err(ApiError::bad_request("an operation cannot be dated in the future"));
            }
            Ok(d)
        }
        None => Ok(today),
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
    let currency = b.currency.trim().to_uppercase();
    if !CURRENCIES.contains(&currency.as_str()) {
        return Err(ApiError::bad_request("unknown currency"));
    }
    let pf = store::create_portfolio(&state.pool, name, b.description.trim(), &currency).await?;
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
    // Above the portfolio's weight limit, flagged on the row. Same rule as the alert.
    let concentration: Vec<Uuid> = match pf.max_weight_pct {
        Some(max) => alerts::over_limit(&alerts::weights(&book), max),
        None => Vec::new(),
    };
    let exposure = exposure(&state, &book).await?;
    Ok(Json(json!({
        "portfolio": pf,
        "summary": summary,
        "book": book,
        "assets": book.positions,
        "operations": operations,
        "snapshots": snapshots,
        "concentration": concentration,
        "exposure": exposure,
    })))
}

/// Sector and country of each holding: what the user typed, else what Fundamentals holds
/// for a stock's ticker. An ETF is never read from a company record: a fund is not its
/// issuer, so it stays unclassified until the user says what it is.
async fn exposure(state: &AppState, book: &store::Book) -> Result<Value, ApiError> {
    let tickers: Vec<String> = book
        .positions
        .iter()
        .filter(|p| p.asset.asset_class == "stock")
        .map(|p| p.asset.symbol.trim().to_uppercase())
        .collect();
    let fund = store::fund_classification(&state.pool, &tickers).await?;
    let mut sector = Vec::new();
    let mut country = Vec::new();
    let mut per_asset = serde_json::Map::new();
    for p in book.positions.iter().filter(|p| p.quantity > 0.0) {
        let Some(mv) = p.market_value else { continue };
        let f = (p.asset.asset_class == "stock")
            .then(|| fund.get(&p.asset.symbol.trim().to_uppercase()))
            .flatten();
        let pick = |own: &str, other: Option<&String>| -> (String, &'static str) {
            if !own.trim().is_empty() {
                (own.trim().to_string(), "user")
            } else if let Some(o) = other.filter(|o| !o.trim().is_empty()) {
                (o.trim().to_string(), "fundamentals")
            } else {
                (String::new(), "none")
            }
        };
        let (sec, sec_src) = pick(&p.asset.sector, f.map(|x| &x.0));
        let (cty, cty_src) = pick(&p.asset.country, f.map(|x| &x.1));
        per_asset.insert(
            p.asset.id.to_string(),
            json!({ "sector": sec, "sector_source": sec_src, "country": cty, "country_source": cty_src }),
        );
        sector.push((sec, mv));
        country.push((cty, mv));
    }
    Ok(json!({
        "sector": overview::group(&sector),
        "country": overview::group(&country),
        "assets": per_asset,
    }))
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
    /// Notify once when a target bucket leaves its band.
    drift_notify: Option<bool>,
    /// Channel ids (granted to portfolios) for drift, concentration and plan alerts. Empty =
    /// every granted channel.
    alert_channels: Option<Vec<Uuid>>,
    /// Largest share of net worth one holding may take, percent. null clears the limit.
    #[serde(default, deserialize_with = "double_option_f64")]
    #[schemars(with = "Option<f64>")]
    max_weight_pct: Option<Option<f64>>,
    concentration_notify: Option<bool>,
    /// off | daily | weekly.
    digest: Option<String>,
    digest_channels: Option<Vec<Uuid>>,
}

/// Every id must be a channel the portfolios module is granted.
async fn validate_channels(state: &AppState, ids: &[Uuid]) -> Result<(), ApiError> {
    for id in ids {
        match otw_store::notif_channels::get(&state.pool, *id).await? {
            Some(ch) if ch.allows("portfolios") => {}
            Some(_) => {
                return Err(ApiError::bad_request(
                    "this notification channel is not allowed for Portfolio Tracker",
                ))
            }
            None => return Err(ApiError::bad_request("unknown notification channel")),
        }
    }
    Ok(())
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
    let touches_alerts = b.drift_notify.is_some()
        || b.alert_channels.is_some()
        || b.max_weight_pct.is_some()
        || b.concentration_notify.is_some()
        || b.digest.is_some()
        || b.digest_channels.is_some();
    if !touches_alerts {
        return Ok(Json(json!({ "portfolio": pf })));
    }
    if let Some(d) = &b.digest {
        if !matches!(d.as_str(), "off" | "daily" | "weekly") {
            return Err(ApiError::bad_request("digest must be off, daily or weekly"));
        }
    }
    if let Some(Some(m)) = b.max_weight_pct {
        if !(m > 0.0 && m <= 100.0) {
            return Err(ApiError::bad_request("max_weight_pct is between 0 and 100"));
        }
    }
    for ids in [&b.alert_channels, &b.digest_channels].into_iter().flatten() {
        validate_channels(&state, ids).await?;
    }
    let pf = store::update_alert_settings(
        &state.pool,
        id,
        &store::AlertSettings {
            drift_notify: b.drift_notify,
            alert_channels: b.alert_channels.clone(),
            max_weight_pct: b.max_weight_pct,
            concentration_notify: b.concentration_notify,
            digest: b.digest.clone(),
            digest_channels: b.digest_channels.clone(),
        },
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchQuery {
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
    if !ASSET_CLASSES.contains(&b.asset_class.trim()) {
        return Err(ApiError::bad_request(&format!(
            "unknown asset_class {:?}; valid: {}",
            b.asset_class,
            ASSET_CLASSES.join(", ")
        )));
    }
    // Never guess an instrument: an id the provider does not know (`BTC` where CoinGecko
    // says `bitcoin`) is refused before a row exists. A source that cannot be reached says
    // nothing about the id, so the asset is still added and priced later, as before.
    let mut warning = None;
    match prices::instrument_known(b.provider.trim(), b.provider_id.trim()).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(ApiError::bad_request(&format!(
                "{} has no price for {:?}: pick the provider_id from GET /api/portfolios/search \
                 (CoinGecko ids are names like \"bitcoin\", not tickers like \"BTC\")",
                b.provider.trim(),
                b.provider_id.trim()
            )));
        }
        Err(e) => {
            tracing::warn!("could not check {} on {}: {e:#}", b.provider_id.trim(), b.provider);
            warning = Some(format!(
                "{} could not be reached to check {:?}; the asset was added unpriced",
                b.provider.trim(),
                b.provider_id.trim()
            ));
        }
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
    match warning {
        Some(w) => Ok(Json(json!({ "asset": asset, "warning": w }))),
        None => Ok(Json(json!({ "asset": asset }))),
    }
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

/// The same, for a number (a limit that can be cleared).
fn double_option_f64<'de, D>(d: D) -> Result<Option<Option<f64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<f64>::deserialize(d)?))
}

/// The same, for a date written YYYY-MM-DD (a review or dividend date that can be cleared).
fn double_option_date<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
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
    /// Why the line is held. Free text.
    thesis: Option<String>,
    /// YYYY-MM-DD to read the thesis again; null clears it.
    #[serde(default, deserialize_with = "double_option_date")]
    #[schemars(with = "Option<String>")]
    review_at: Option<Option<String>>,
    /// Paid per share per payment, in the asset currency; null clears it.
    #[serde(default, deserialize_with = "double_option_f64")]
    #[schemars(with = "Option<f64>")]
    div_per_share: Option<Option<f64>>,
    /// none | monthly | quarterly | semi | annual.
    div_frequency: Option<String>,
    #[serde(default, deserialize_with = "double_option_date")]
    #[schemars(with = "Option<String>")]
    div_next_ex: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option_date")]
    #[schemars(with = "Option<String>")]
    div_next_pay: Option<Option<String>>,
    /// Sector and country, free text. Empty = read from Fundamentals for a stock.
    sector: Option<String>,
    country: Option<String>,
}

fn opt_date(v: &Option<Option<String>>) -> Result<Option<Option<Date>>, ApiError> {
    match v {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(s)) if s.trim().is_empty() => Ok(Some(None)),
        Some(Some(s)) => Date::parse(s.trim(), &Iso8601::DATE)
            .map(|d| Some(Some(d)))
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
    }
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
    let profile = store::AssetProfile {
        thesis: b.thesis.as_deref().map(|t| t.trim().to_string()),
        review_at: opt_date(&b.review_at)?,
        div_per_share: b.div_per_share,
        div_frequency: b.div_frequency.as_deref().map(|f| f.trim().to_string()),
        div_next_ex: opt_date(&b.div_next_ex)?,
        div_next_pay: opt_date(&b.div_next_pay)?,
        sector: b.sector.as_deref().map(|t| t.trim().to_string()),
        country: b.country.as_deref().map(|t| t.trim().to_string()),
    };
    if let Some(f) = &profile.div_frequency {
        if !store::DIV_FREQUENCIES.contains(&f.as_str()) {
            return Err(ApiError::bad_request("div_frequency must be none, monthly, quarterly, semi or annual"));
        }
    }
    if let Some(Some(v)) = profile.div_per_share {
        if !(v.is_finite() && v >= 0.0) {
            return Err(ApiError::bad_request("div_per_share cannot be negative"));
        }
    }
    let touched = profile.thesis.is_some()
        || profile.review_at.is_some()
        || profile.div_per_share.is_some()
        || profile.div_frequency.is_some()
        || profile.div_next_ex.is_some()
        || profile.div_next_pay.is_some()
        || profile.sector.is_some()
        || profile.country.is_some();
    let asset = if touched {
        store::update_asset_profile(&state.pool, asset_id, &profile)
            .await?
            .ok_or_else(|| ApiError::not_found("asset not found"))?
    } else {
        asset
    };
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
    /// split / reverse_split: `ratio_num` new units for `ratio_den` old ones (2 for 1 is 2/1,
    /// 1 for 10 is 1/10).
    #[serde(default)]
    ratio_num: Option<f64>,
    #[serde(default)]
    ratio_den: Option<f64>,
    /// spin_off: the asset of the same portfolio that receives the new units.
    #[serde(default)]
    child_asset_id: Option<Uuid>,
    #[serde(default)]
    child_qty: Option<f64>,
    /// spin_off: percent of the parent's cost basis that moves to the child (0..100).
    #[serde(default)]
    cost_share_pct: Option<f64>,
}

/// How far below zero the position would go, on any date from the sale on, with a sale of
/// `qty` booked on `date` (`None` when it never does). A ledger holds no short positions:
/// selling what was never bought is a missing buy, not a phantom gain. Splits, spin-offs
/// and transfers count: the floor is read through the position engine.
fn oversold(ops: &[store::Operation], asset: Uuid, date: Date, qty: f64) -> Option<f64> {
    let mut rows: Vec<otw_store::portfolios::ledger::LedgerOp> =
        ops.iter().filter_map(|o| o.ledger_op(0.0, 0.0)).collect();
    rows.push(otw_store::portfolios::ledger::LedgerOp {
        asset_id: asset,
        side: "sell".into(),
        date,
        qty,
        gross: 0.0,
        fee: 0.0,
        ratio: None,
        child: None,
        lot_date: None,
    });
    let worst = otw_store::portfolios::ledger::min_qty_from(&rows, asset, date);
    (worst < -1e-9).then_some(-worst)
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
            "unknown operation kind (buy, sell, deposit, withdraw, dividend, interest, coupon, fee, tax, split, reverse_split, spin_off)",
        ));
    }
    let asset_id = b.asset_id.or_else(|| asset.as_ref().map(|a| a.id));
    if store::is_corporate(side) {
        let Some(asset_id) = asset_id else {
            return Err(ApiError::bad_request("a corporate action needs an asset"));
        };
        return book_corporate(state, pf, asset_id, side, &b).await;
    }
    if store::is_trade(side) && asset_id.is_none() {
        return Err(ApiError::bad_request("a buy or a sell needs an asset"));
    }
    // The amount lives in `price` with a quantity of 1 for every cash kind, so one shape
    // covers the whole ledger and the walk never branches to read it.
    let (quantity, price) = if store::is_trade(side) {
        if !(b.quantity.is_finite() && b.quantity > 0.0) {
            return Err(ApiError::bad_request("quantity must be positive"));
        }
        // Zero stays allowed: shares received for nothing (a gift, a spin-off) cost nothing.
        if !(b.price.is_finite() && b.price >= 0.0) {
            return Err(ApiError::bad_request("price cannot be negative"));
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
    if side == "sell" {
        if let Some(asset_id) = asset_id {
            let ops = store::asset_ledger(&state.pool, asset_id).await?;
            if let Some(short) = oversold(&ops, asset_id, date, quantity) {
                return Err(ApiError::bad_request(&format!(
                    "this sale is {short} more than the position holds on that date: book the buy it closes first"
                )));
            }
        }
    }

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

/// Validate and book a split, a reverse split or a spin-off. Nothing about cash: these rows
/// change units or cost basis only.
async fn book_corporate(
    state: &AppState,
    pf: &store::Portfolio,
    asset_id: Uuid,
    side: &str,
    b: &AddOpBody,
) -> Result<store::Operation, ApiError> {
    let date = parse_date(b.op_date.as_deref())?;
    let rows: Vec<_> = store::asset_ledger(&state.pool, asset_id)
        .await?
        .iter()
        .filter_map(|o| o.ledger_op(0.0, 0.0))
        .collect();
    let held = otw_store::portfolios::ledger::qty_on(&rows, asset_id, date);
    if held <= 1e-9 {
        return Err(ApiError::bad_request("the position holds no units on that date"));
    }
    let (ratio, child) = match side {
        "split" | "reverse_split" => {
            let (Some(n), Some(d)) = (b.ratio_num, b.ratio_den) else {
                return Err(ApiError::bad_request("a split needs ratio_num and ratio_den"));
            };
            if !(n.is_finite() && d.is_finite() && n > 0.0 && d > 0.0) {
                return Err(ApiError::bad_request("a split ratio is two positive numbers"));
            }
            if side == "split" && n <= d {
                return Err(ApiError::bad_request("a split gives more units than it takes (ratio_num > ratio_den)"));
            }
            if side == "reverse_split" && n >= d {
                return Err(ApiError::bad_request("a reverse split gives fewer units than it takes (ratio_num < ratio_den)"));
            }
            (Some((n, d)), None)
        }
        _ => {
            let Some(child_id) = b.child_asset_id else {
                return Err(ApiError::bad_request("a spin-off needs the asset that receives the units"));
            };
            if child_id == asset_id {
                return Err(ApiError::bad_request("a spin-off delivers units of another asset"));
            }
            let child = store::get_asset(&state.pool, child_id)
                .await?
                .ok_or_else(|| ApiError::not_found("spin-off asset not found"))?;
            if child.portfolio_id != pf.id {
                return Err(ApiError::bad_request("the spin-off asset belongs to another portfolio"));
            }
            let qty = b.child_qty.unwrap_or(0.0);
            if !(qty.is_finite() && qty > 0.0) {
                return Err(ApiError::bad_request("child_qty must be positive"));
            }
            let pct = b.cost_share_pct.unwrap_or(-1.0);
            if !(0.0..=100.0).contains(&pct) {
                return Err(ApiError::bad_request("cost_share_pct is between 0 and 100"));
            }
            (None, Some((child_id, qty, pct)))
        }
    };
    let op = store::add_corporate_action(&state.pool, pf.id, asset_id, side, date, ratio, child, b.note.trim()).await?;
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

// ── All portfolios ───────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct OverviewQuery {
    /// Currency every book is summed in. Defaults to the first portfolio's.
    currency: Option<String>,
}

/// Every book summed in one currency: value, cost, P/L, the same symbol held in several
/// books, currency exposure and the largest lines.
async fn overview(
    State(state): State<AppState>,
    Query(q): Query<OverviewQuery>,
) -> Result<Json<Value>, ApiError> {
    let portfolios = store::list_portfolios(&state.pool).await?;
    let currency = match q.currency.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) if CURRENCIES.contains(&c) => c.to_string(),
        Some(_) => return Err(ApiError::bad_request("unknown currency")),
        None => portfolios.first().map(|p| p.currency.clone()).unwrap_or_else(|| "USD".into()),
    };
    let today = time::OffsetDateTime::now_utc().date();
    let mut fx = otw_store::journal_fx::FxCache::new();
    let mut held = Vec::new();
    let (mut value, mut cost, mut unrealized, mut realized, mut cash) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut books = Vec::new();
    for pf in &portfolios {
        let b = store::book(&state.pool, pf).await?;
        // A book's figures are in its own currency: one rate per book, today's.
        let rate = if pf.currency == currency {
            1.0
        } else {
            fx.convert(&state.pool, 1.0, &pf.currency, &currency, today).await?.unwrap_or(1.0)
        };
        value += b.market_value * rate;
        cost += b.cost_basis * rate;
        unrealized += b.unrealized * rate;
        realized += b.realized * rate;
        cash += b.cash_total * rate;
        books.push(json!({ "id": pf.id, "name": pf.name, "net_worth": b.net_worth * rate }));
        for p in b.positions.iter().filter(|p| p.quantity > 0.0) {
            if let Some(mv) = p.market_value {
                held.push(overview::Held {
                    portfolio: pf.name.clone(),
                    symbol: p.asset.symbol.clone(),
                    name: p.asset.name.clone(),
                    currency: p.asset.currency.clone(),
                    value: mv * rate,
                });
            }
        }
    }
    let agg = overview::aggregate(&held);
    Ok(Json(json!({
        "currency": currency,
        "portfolios": books,
        "market_value": value,
        "cost_basis": cost,
        "unrealized": unrealized,
        "realized": realized,
        "cash_total": cash,
        "net_worth": value + cash,
        "overlaps": agg.overlaps,
        "currencies": agg.currencies,
        "top": agg.top,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ReviewsQuery {
    /// Days ahead to include. Default 7.
    days: Option<i64>,
}

/// Assets whose thesis review is due (or due within `days`), soonest first.
async fn reviews_due(
    State(state): State<AppState>,
    Query(q): Query<ReviewsQuery>,
) -> Result<Json<Value>, ApiError> {
    let days = q.days.unwrap_or(7).clamp(0, 366);
    let until = time::OffsetDateTime::now_utc().date() + time::Duration::days(days);
    let rows = store::reviews_due(&state.pool, until).await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(a, name)| json!({
            "asset_id": a.id,
            "portfolio_id": a.portfolio_id,
            "portfolio": name,
            "symbol": a.symbol,
            "name": a.name,
            "review_at": a.review_at.map(|d| d.to_string()),
            "thesis": a.thesis,
        }))
        .collect();
    Ok(Json(json!({ "reviews": out })))
}

// ── Income calendar ───────────────────────────────────────────────────────────

/// Forecast income per holding from the user's dividend data, the next dates, and the
/// income booked over the last twelve months beside it.
async fn income_calendar(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let pf = store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let today = time::OffsetDateTime::now_utc().date();
    let book = store::book(&state.pool, &pf).await?;
    let trailing = store::income_by_asset_since(&state.pool, &pf, today - time::Duration::days(365)).await?;
    let mut fx = otw_store::journal_fx::FxCache::new();
    let mut rows = Vec::new();
    for p in book.positions.iter().filter(|p| p.quantity > 0.0) {
        let a = &p.asset;
        let per_share_pf = match a.div_per_share {
            Some(v) if a.currency != pf.currency => {
                Some(fx.convert(&state.pool, v, &a.currency, &pf.currency, today).await?.unwrap_or(v))
            }
            other => other,
        };
        rows.push(income::forecast(&income::Holding {
            asset_id: a.id,
            symbol: a.symbol.clone(),
            quantity: p.quantity,
            cost_basis: p.cost_basis,
            per_share_pf,
            frequency: a.div_frequency.clone(),
            next_ex: a.div_next_ex,
            next_pay: a.div_next_pay,
            trailing_12m: trailing.get(&Some(a.id)).copied().unwrap_or(0.0),
        }));
    }
    let forecast_total: f64 = rows.iter().filter_map(|r| r.annual).sum();
    let no_estimate: Vec<&str> = rows.iter().filter(|r| r.annual.is_none()).map(|r| r.symbol.as_str()).collect();
    let trailing_total: f64 = trailing.values().sum();
    let events = income::upcoming(&rows, today);
    Ok(Json(json!({
        "currency": pf.currency,
        "holdings": rows,
        "events": events,
        "forecast_annual": forecast_total,
        "trailing_12m": trailing_total,
        "yield_on_cost_pct": (book.cost_basis > 0.0 && forecast_total > 0.0).then(|| forecast_total / book.cost_basis * 100.0),
        "no_estimate": no_estimate,
    })))
}

// ── Lots ──────────────────────────────────────────────────────────────────────

async fn asset_in(state: &AppState, pf_id: Uuid, asset_id: Uuid) -> Result<(store::Portfolio, store::Asset), ApiError> {
    let pf = store::get_portfolio(&state.pool, pf_id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let asset = store::get_asset(&state.pool, asset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("asset not found"))?;
    if asset.portfolio_id != pf.id {
        return Err(ApiError::bad_request("that asset belongs to another portfolio"));
    }
    Ok((pf, asset))
}

/// Open lots of one asset under the Tax Calculator's cost method.
async fn asset_lots(
    State(state): State<AppState>,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let (pf, asset) = asset_in(&state, id, asset_id).await?;
    let today = time::OffsetDateTime::now_utc().date();
    let rules = lots::tax_rules(&state.pool, today).await;
    let (ledger, _) = lots::walk(&state.pool, &pf, rules.method).await?;
    let price = lots::price_of(&state.pool, &pf, &asset, today).await?;
    let open = ledger.position(&asset.id).map(|p| p.lots.clone()).unwrap_or_default();
    let views = lots::lot_views(&asset, &open, price, &rules, today);
    Ok(Json(json!({
        "currency": pf.currency,
        "rules": rules.describe(),
        "long_after_days": rules.long_after(&asset.asset_class),
        "lots": views,
        "realized": ledger.position(&asset.id).map(|p| p.realized).unwrap_or(0.0),
    })))
}

// ── Transfer ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TransferBody {
    to_portfolio_id: Uuid,
    qty: f64,
    /// YYYY-MM-DD, default today.
    #[serde(default)]
    op_date: Option<String>,
    #[serde(default)]
    note: String,
}

/// Move units to another portfolio with their lots and cost: nothing is realized. The
/// destination asset is the same instrument (provider and id), created when missing.
async fn transfer(
    State(state): State<AppState>,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<TransferBody>,
) -> Result<Json<Value>, ApiError> {
    let (pf, asset) = asset_in(&state, id, asset_id).await?;
    if b.to_portfolio_id == pf.id {
        return Err(ApiError::bad_request("pick another portfolio to transfer to"));
    }
    let dest = store::get_portfolio(&state.pool, b.to_portfolio_id)
        .await?
        .ok_or_else(|| ApiError::not_found("destination portfolio not found"))?;
    if !(b.qty.is_finite() && b.qty > 0.0) {
        return Err(ApiError::bad_request("qty must be positive"));
    }
    let date = parse_date(b.op_date.as_deref())?;
    // A transfer is booked on today's book: a backdated one would have to move lots that
    // were sold since, and the ledger would no longer say what happened.
    let latest = store::asset_ledger(&state.pool, asset.id).await?.iter().map(|o| o.op_date).max();
    if latest.is_some_and(|l| l > date) {
        return Err(ApiError::bad_request("a transfer cannot be dated before the asset's last operation"));
    }
    let today = time::OffsetDateTime::now_utc().date();
    let method = lots::tax_rules(&state.pool, today).await.method;
    let (ledger, _) = lots::walk(&state.pool, &pf, method).await?;
    let Some(pos) = ledger.position(&asset.id) else {
        return Err(ApiError::bad_request("the position holds no units"));
    };
    if b.qty > pos.qty() + 1e-9 {
        return Err(ApiError::bad_request(&format!(
            "the position holds {} units, fewer than the {} asked",
            pos.qty(),
            b.qty
        )));
    }
    let moved = pos.peek(b.qty);
    let to_asset = store::add_asset(
        &state.pool,
        dest.id,
        &asset.asset_class,
        &asset.provider,
        &asset.provider_id,
        &asset.symbol,
        &asset.name,
        &asset.currency,
    )
    .await?;
    if to_asset.hist_symbol.is_empty() && !asset.hist_symbol.is_empty() {
        store::update_asset(
            &state.pool,
            to_asset.id,
            None,
            None,
            None,
            Some(&asset.hist_symbol),
            Some(&asset.hist_currency),
            None,
        )
        .await?;
    }
    if to_asset.last_price_usd.is_none() {
        if let Some(p) = asset.last_price_usd {
            store::set_asset_price(&state.pool, to_asset.id, p).await?;
        }
    }
    let transfer_id = store::book_transfer(
        &state.pool,
        pf.id,
        asset.id,
        dest.id,
        to_asset.id,
        date,
        &moved,
        &pf.currency,
        b.note.trim(),
    )
    .await?;
    store::mark_rebuild_from(&state.pool, pf.id, date).await?;
    store::mark_rebuild_from(&state.pool, dest.id, date).await?;
    Ok(Json(json!({
        "transfer_id": transfer_id,
        "to_asset_id": to_asset.id,
        "lots": moved,
        "qty": b.qty,
    })))
}

// ── Planned trades ────────────────────────────────────────────────────────────

async fn plans_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "plans": store::list_plans(&state.pool, id).await? })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PlanBody {
    asset_id: Uuid,
    /// buy | sell.
    side: String,
    /// Level in the asset's own currency.
    price: f64,
    #[serde(default)]
    qty: Option<f64>,
    #[serde(default)]
    note: String,
    /// Send a notification when the level is reached. Default true.
    #[serde(default = "yes")]
    notify: bool,
}
fn yes() -> bool {
    true
}

/// A buy or a sell intended at a price. The asset's quote is checked every hour and the
/// plan is marked reached once, with a notification when asked.
async fn plan_add(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<PlanBody>,
) -> Result<Json<Value>, ApiError> {
    let (pf, asset) = asset_in(&state, id, b.asset_id).await?;
    if !matches!(b.side.as_str(), "buy" | "sell") {
        return Err(ApiError::bad_request("side must be buy or sell"));
    }
    if !(b.price.is_finite() && b.price > 0.0) {
        return Err(ApiError::bad_request("price must be positive"));
    }
    if let Some(q) = b.qty {
        if !(q.is_finite() && q > 0.0) {
            return Err(ApiError::bad_request("qty must be positive"));
        }
    }
    let plan = store::add_plan(&state.pool, pf.id, asset.id, &b.side, b.price, b.qty, b.note.trim(), b.notify).await?;
    Ok(Json(json!({ "plan": plan })))
}

async fn plan_delete(
    State(state): State<AppState>,
    Path((id, plan_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_plan(&state.pool, id, plan_id).await? {
        return Err(ApiError::not_found("plan not found"));
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

    #[test]
    fn a_plan_maps_to_its_level_check() {
        // A buy plan at 100 is reached by a quote of 100 or less, never by 101.
        assert!(alerts::plan_hit("buy", 100.0, 100.0));
        assert!(!alerts::plan_hit("buy", 100.0, 101.0));
        assert!(alerts::plan_hit("sell", 100.0, 101.0));
    }

    /// Clock skew (a timestamp in the future) must not lock the user out.
    #[test]
    fn future_timestamp_is_allowed() {
        assert_eq!(cooldown_remaining(Some(now() + Duration::hours(1)), now()), None);
    }
}
