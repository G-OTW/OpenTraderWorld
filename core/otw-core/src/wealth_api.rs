//! HTTP API for the MyWealth module.
//!
//! Asset templates (reserved + custom fields), assets, revisions (each "Update" adds one),
//! a net-worth time-series breakdown, and the display-currency setting. On top of those:
//! loan schedules, live broker and wallet readings, goals, movers, snapshots, category
//! targets, the CPI-deflated curve's inputs, attachments and the CSV export. The math lives
//! in `crate::wealth`.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date, OffsetDateTime};
use uuid::Uuid;

use crate::wealth as engine;
use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::wealth::{
    self, AssetFilter, AssetInput, RevisionInput, TemplateInput, TemplatePatch, ASSET_TYPES,
    SOURCE_LOAN,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/wealth/templates", get(list_templates).post(add_template))
        .route(
            "/api/wealth/templates/{id}",
            axum::routing::patch(update_template).delete(delete_template),
        )
        .route("/api/wealth/assets", get(list_assets).post(add_asset))
        .route(
            "/api/wealth/assets/{id}",
            axum::routing::patch(update_asset).delete(delete_asset),
        )
        .route(
            "/api/wealth/assets/{id}/revisions",
            get(list_revisions).post(add_revision),
        )
        .route(
            "/api/wealth/revisions/{id}",
            axum::routing::patch(update_revision).delete(delete_revision),
        )
        .route("/api/wealth/breakdown", get(breakdown))
        .route("/api/wealth/settings", get(get_settings).patch(update_settings))
        .route("/api/wealth/preferences", axum::routing::put(put_preferences))
        .route("/api/wealth/assets/{id}/schedule", get(loan_schedule))
        .route("/api/wealth/goals", get(goals))
        .route("/api/wealth/movers", get(movers))
        .route("/api/wealth/snapshot", get(snapshot))
        .route("/api/wealth/targets", get(get_targets).put(put_targets))
        .route("/api/wealth/categories", get(list_categories).put(put_category))
        .route("/api/wealth/real", get(real_inputs))
        .route("/api/wealth/channels", get(channels))
        .route(
            "/api/wealth/assets/{id}/attachments",
            get(list_attachments)
                .post(upload_attachment)
                .layer(DefaultBodyLimit::max(engine::MAX_ATTACHMENT + 1024 * 1024)),
        )
        .route("/api/wealth/attachments/{id}", get(download_attachment).delete(delete_attachment))
        .route("/api/wealth/export.csv", get(export_csv))
}

// ── Templates ──

async fn list_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let templates = wealth::list_templates(&state.pool).await?;
    Ok(Json(json!({ "templates": templates })))
}

fn check_type(t: &str) -> Result<(), ApiError> {
    if !ASSET_TYPES.contains(&t) {
        return Err(ApiError::bad_request("unknown asset type"));
    }
    Ok(())
}

async fn add_template(
    State(state): State<AppState>,
    Json(mut input): Json<TemplateInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        input.name = "Untitled template".to_string();
    }
    check_type(&input.asset_type)?;
    let id = wealth::add_template(&state.pool, &input).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<TemplatePatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(t) = &patch.asset_type {
        check_type(t)?;
    }
    if !wealth::update_template(&state.pool, id, &patch).await? {
        return Err(ApiError::bad_request("template not found or is built-in"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_template(&state.pool, id).await? {
        return Err(ApiError::bad_request("template not found or is built-in"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Assets ──

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AssetQuery {
    asset_type: Option<String>,
    category: Option<String>,
    /// Comma-separated owners; a bare "," is the no-owner bucket.
    owner: Option<String>,
}

/// Filter params are comma-separated lists, so a single value stays a valid query
/// (`?category=Cash`) while the UI can send several (`?category=Cash,Stocks`).
/// A bare `,` entry is a real value: the uncategorized bucket.
fn filter_list(raw: Option<String>) -> Vec<String> {
    raw.map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .filter(|v: &Vec<String>| !(v.len() == 1 && v[0].is_empty()))
        .unwrap_or_default()
}

async fn list_assets(
    State(state): State<AppState>,
    Query(q): Query<AssetQuery>,
) -> Result<Json<Value>, ApiError> {
    // Live assets (broker accounts, wallets) are read before the list, at most hourly, so the
    // latest revision the row shows is the reading. A failure stays on the row.
    if let Err(e) = engine::refresh_live(&state.pool, &state.cipher).await {
        tracing::warn!("wealth: live refresh failed: {e:#}");
    }
    let filter = AssetFilter {
        asset_types: filter_list(q.asset_type),
        categories: filter_list(q.category),
        owners: filter_list(q.owner),
    };
    let assets = wealth::list_assets(&state.pool, &filter).await?;
    let errors = engine::live_errors();
    let today = OffsetDateTime::now_utc().date();
    let rows: Vec<Value> = assets
        .iter()
        .map(|a| {
            let mut v = serde_json::to_value(a).unwrap_or(Value::Null);
            if let Value::Object(m) = &mut v {
                m.insert("live_error".into(), json!(errors.get(&a.id)));
                // Today's value under the owner's assumed drift, shown apart from the recorded
                // one so the row never passes an estimate off as a valuation.
                let estimate = match (a.drift_pct_year, a.latest_value, a.latest_at) {
                    (Some(rate), Some(v), Some(at)) if !a.is_automatic() && rate != 0.0 && today > at => {
                        Some(wealth::drifted(v, rate, (today - at).whole_days()))
                    }
                    _ => None,
                };
                m.insert("estimated_value".into(), json!(estimate));
            }
            v
        })
        .collect();
    Ok(Json(json!({ "assets": rows })))
}

fn validate_asset(input: &AssetInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("asset name required"));
    }
    check_type(&input.asset_type)?;
    if !CURRENCIES.contains(&input.currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    if !matches!(input.sign, -1 | 1) {
        return Err(ApiError::bad_request("sign must be 1 (owned) or -1 (owed)"));
    }
    if input.review_days < 0 {
        return Err(ApiError::bad_request("a review interval cannot be negative"));
    }
    // A linked asset is valued from the portfolio it points at, so it cannot also be a debt:
    // a portfolio's net worth already carries its own sign.
    if input.portfolio_id.is_some() && input.sign != 1 {
        return Err(ApiError::bad_request("a linked portfolio is not a liability"));
    }
    if let Some(terms) = &input.loan {
        if input.asset_type != "loan" || input.sign != -1 {
            return Err(ApiError::bad_request("loan terms belong to an owed asset of type loan"));
        }
        engine::loan::Loan::parse(terms).map_err(|e| ApiError::bad_request(&e))?;
    }
    if let Some(rate) = input.drift_pct_year {
        if !rate.is_finite() || rate <= -100.0 || rate > 100.0 {
            return Err(ApiError::bad_request("a yearly drift must be above -100 % and at most 100 %"));
        }
        if input.portfolio_id.is_some() || input.broker_account_id.is_some() || input.address.is_some() || input.loan.is_some() {
            return Err(ApiError::bad_request("drift only applies to an asset you value yourself"));
        }
    }
    match input.asset_type.as_str() {
        "broker_account" => {
            if input.broker_account_id.is_none() {
                return Err(ApiError::bad_request("pick the broker account this asset reads"));
            }
            if input.sign != 1 {
                return Err(ApiError::bad_request("a broker account's equity is not a liability"));
            }
        }
        "wallet" => {
            let (Some(chain), Some(addr)) = (input.chain.as_deref(), input.address.as_deref()) else {
                return Err(ApiError::bad_request("a wallet needs a chain and an address"));
            };
            engine::wallet::validate_address(chain, addr).map_err(|e| ApiError::bad_request(&e))?;
            if input.sign != 1 {
                return Err(ApiError::bad_request("a wallet balance is not a liability"));
            }
        }
        _ => {
            if input.broker_account_id.is_some() || input.address.is_some() {
                return Err(ApiError::bad_request("only a broker_account or wallet asset is read live"));
            }
        }
    }
    Ok(())
}

/// Trim the free-text fields and drop what the asset's kind does not use.
fn normalize(input: &mut AssetInput) {
    input.name = input.name.trim().to_string();
    input.owner = input.owner.take().map(|o| o.trim().to_string()).filter(|o| !o.is_empty());
    input.address = input.address.take().map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    input.chain = input.chain.take().map(|c| c.trim().to_ascii_lowercase()).filter(|c| !c.is_empty());
    if input.asset_type != "wallet" {
        input.chain = None;
        input.address = None;
    }
    if input.asset_type != "broker_account" {
        input.broker_account_id = None;
    }
    // A value the module reads or computes cannot age.
    if input.broker_account_id.is_some() || input.address.is_some() || input.loan.is_some() {
        input.review_days = 0;
    }
    if input.loan.as_ref().is_some_and(Value::is_null) {
        input.loan = None;
    }
}

/// Checks that need the database: the paired debt and the broker account.
async fn check_links(state: &AppState, id: Option<Uuid>, input: &AssetInput) -> Result<(), ApiError> {
    if let Some(pid) = input.paired_liability_id {
        if Some(pid) == id {
            return Err(ApiError::bad_request("an asset cannot be paired with itself"));
        }
        if input.sign != 1 {
            return Err(ApiError::bad_request("only an owned asset is paired with a debt"));
        }
        let debt = wealth::get_asset(&state.pool, pid)
            .await?
            .ok_or_else(|| ApiError::bad_request("the paired liability does not exist"))?;
        if debt.sign != -1 {
            return Err(ApiError::bad_request("the paired asset must be owed (a liability)"));
        }
    }
    if let Some(acc) = input.broker_account_id {
        let account = otw_store::brokers::get(&state.pool, acc)
            .await?
            .ok_or_else(|| ApiError::bad_request("broker account not found"))?;
        if !account.allows(engine::broker::MODULE) {
            return Err(ApiError::bad_request(&format!(
                "“{}” is not granted to MyWealth. Allow it in Settings, Brokers.",
                account.name
            )));
        }
    }
    Ok(())
}

/// After a save: rewrite the loan schedule (or clear it), and read a live source anew.
async fn after_save(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    engine::forget_reading(id);
    let Some(asset) = wealth::get_asset(&state.pool, id).await? else {
        return Ok(());
    };
    if asset.loan.is_some() {
        engine::sync_loan(&state.pool, &asset, true).await?;
    } else {
        wealth::replace_auto_revisions(&state.pool, id, SOURCE_LOAN, &[], "").await?;
    }
    Ok(())
}

async fn add_asset(
    State(state): State<AppState>,
    Json(mut input): Json<AssetInput>,
) -> Result<Json<Value>, ApiError> {
    normalize(&mut input);
    validate_asset(&input)?;
    check_links(&state, None, &input).await?;
    let id = wealth::add_asset(&state.pool, &input).await?;
    after_save(&state, id).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<AssetInput>,
) -> Result<Json<Value>, ApiError> {
    normalize(&mut input);
    validate_asset(&input)?;
    check_links(&state, Some(id), &input).await?;
    if !wealth::update_asset(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("asset not found"));
    }
    after_save(&state, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_asset(&state.pool, id).await? {
        return Err(ApiError::not_found("asset not found"));
    }
    engine::forget_reading(id);
    Ok(Json(json!({ "ok": true })))
}

// ── Revisions ──

async fn list_revisions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let revisions = wealth::list_revisions(&state.pool, id).await?;
    Ok(Json(json!({ "revisions": revisions })))
}

async fn add_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RevisionInput>,
) -> Result<Json<Value>, ApiError> {
    let rev_id = wealth::add_revision(&state.pool, id, &input).await?;
    Ok(Json(json!({ "id": rev_id })))
}

async fn update_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RevisionInput>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::update_revision(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("revision not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_revision(&state.pool, id).await? {
        return Err(ApiError::not_found("revision not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Breakdown + settings ──

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BreakdownQuery {
    asset_type: Option<String>,
    category: Option<String>,
    owner: Option<String>,
    #[serde(default = "default_points")]
    points_back: i64,
    /// "month" (default) or "year".
    granularity: Option<String>,
}
fn default_points() -> i64 {
    11
}

async fn breakdown(
    State(state): State<AppState>,
    Query(q): Query<BreakdownQuery>,
) -> Result<Json<Value>, ApiError> {
    if let Err(e) = engine::refresh_live(&state.pool, &state.cipher).await {
        tracing::warn!("wealth: live refresh failed: {e:#}");
    }
    let filter = AssetFilter {
        asset_types: filter_list(q.asset_type),
        categories: filter_list(q.category),
        owners: filter_list(q.owner),
    };
    let granularity = match q.granularity.as_deref() {
        Some("year") => "year",
        _ => "month",
    };
    let settings = wealth::get_settings(&state.pool).await?;
    let data = wealth::breakdown(
        &state.pool,
        &filter,
        &settings.display_currency,
        q.points_back.clamp(0, 60),
        granularity,
    )
    .await?;
    Ok(Json(json!({ "breakdown": data })))
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = wealth::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SettingsBody {
    display_currency: Option<String>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = body.display_currency {
        if !CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
        wealth::set_display_currency(&state.pool, &c).await?;
    }
    let settings = wealth::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

// ── Preferences: goals, stale notice, CPI series ──

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PreferencesBody {
    goal_value: Option<f64>,
    /// `YYYY-MM-DD`.
    goal_date: Option<String>,
    monthly_expenses: Option<f64>,
    /// `off` | `weekly`.
    #[serde(default = "off")]
    stale_notify: String,
    #[serde(default)]
    stale_channels: Vec<Uuid>,
    cpi_series_id: Option<Uuid>,
}
fn off() -> String {
    "off".into()
}

fn parse_date(raw: Option<&str>, what: &str) -> Result<Option<Date>, ApiError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map(Some)
            .map_err(|_| ApiError::bad_request(&format!("{what} must be a YYYY-MM-DD date"))),
    }
}

async fn put_preferences(
    State(state): State<AppState>,
    Json(b): Json<PreferencesBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(b.stale_notify.as_str(), "off" | "weekly") {
        return Err(ApiError::bad_request("stale notice must be off or weekly"));
    }
    for (v, what) in [(b.goal_value, "goal"), (b.monthly_expenses, "monthly expenses")] {
        if v.is_some_and(|x| !x.is_finite() || x < 0.0) {
            return Err(ApiError::bad_request(&format!("{what} cannot be negative")));
        }
    }
    let mut s = wealth::get_settings(&state.pool).await?;
    s.goal_value = b.goal_value;
    s.goal_date = parse_date(b.goal_date.as_deref(), "goal date")?;
    s.monthly_expenses = b.monthly_expenses;
    s.stale_notify = b.stale_notify;
    s.stale_channels = b.stale_channels;
    s.cpi_series_id = b.cpi_series_id;
    wealth::set_preferences(&state.pool, &s).await?;
    let settings = wealth::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

/// Channels the stale notice can go to: those granted to MyWealth.
async fn channels(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let channels = otw_store::notif_channels::list_for_module(&state.pool, engine::MODULE).await?;
    Ok(Json(json!({ "channels": channels })))
}

// ── Loan schedule ──

async fn loan_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let asset = wealth::get_asset(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("asset not found"))?;
    let terms = asset.loan.as_ref().ok_or_else(|| ApiError::bad_request("this asset has no loan terms"))?;
    let loan = engine::loan::Loan::parse(terms).map_err(|e| ApiError::bad_request(&e))?;
    let today = OffsetDateTime::now_utc().date();
    let rows = loan.schedule(today);
    let interest_left: f64 = rows.iter().map(|r| r.interest).sum();
    Ok(Json(json!({
        "currency": asset.currency,
        "payment": loan.payment(),
        "balance": loan.balance_on(today),
        "payments_left": rows.len(),
        "interest_left": interest_left,
        "rows": rows,
    })))
}

// ── Valuations at a date ──

/// The whole book (no filter) valued at each date in the display currency.
async fn valued_on(state: &AppState, dates: &[Date]) -> Result<(String, Vec<Vec<wealth::AssetAt>>), ApiError> {
    let settings = wealth::get_settings(&state.pool).await?;
    let assets = wealth::list_assets(&state.pool, &AssetFilter::default()).await?;
    let values = wealth::load_values(&state.pool, &assets).await?;
    let mut out = Vec::with_capacity(dates.len());
    for d in dates {
        out.push(wealth::values_at(&state.pool, &assets, &values, *d, &settings.display_currency).await?);
    }
    Ok((settings.display_currency, out))
}

async fn valued_at(state: &AppState, date: Date) -> Result<(String, Vec<wealth::AssetAt>), ApiError> {
    let (currency, mut rows) = valued_on(state, &[date]).await?;
    Ok((currency, rows.pop().unwrap_or_default()))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SnapshotQuery {
    /// `YYYY-MM-DD`; today when omitted.
    at: Option<String>,
}

/// Every asset and category at a date: the latest revision on or before it.
async fn snapshot(
    State(state): State<AppState>,
    Query(q): Query<SnapshotQuery>,
) -> Result<Json<Value>, ApiError> {
    let today = OffsetDateTime::now_utc().date();
    let at = parse_date(q.at.as_deref(), "at")?.unwrap_or(today).min(today);
    let (currency, rows) = valued_at(&state, at).await?;
    let net_worth: f64 = rows.iter().filter_map(|r| r.value).sum();
    Ok(Json(json!({
        "at": at.to_string(),
        "display_currency": currency,
        "net_worth": net_worth,
        "by_category": wealth::by_category(&rows),
        "unconverted": rows.iter().filter(|r| r.unconverted).count(),
        "assets": rows,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct MoversQuery {
    /// `YYYY-MM-DD`; a month before `to` when omitted.
    from: Option<String>,
    /// `YYYY-MM-DD`; today when omitted.
    to: Option<String>,
}

/// Which categories moved the net worth between two dates, split into revaluation and
/// additions or removals.
async fn movers(
    State(state): State<AppState>,
    Query(q): Query<MoversQuery>,
) -> Result<Json<Value>, ApiError> {
    let today = OffsetDateTime::now_utc().date();
    let to = parse_date(q.to.as_deref(), "to")?.unwrap_or(today).min(today);
    let from = parse_date(q.from.as_deref(), "from")?.unwrap_or_else(|| wealth::month_end_back(to, 1));
    if from >= to {
        return Err(ApiError::bad_request("from must be before to"));
    }
    let (currency, mut both) = valued_on(&state, &[from, to]).await?;
    let end = both.pop().unwrap_or_default();
    let start = both.pop().unwrap_or_default();
    let m = engine::movers::attribute(&start, &end);
    Ok(Json(json!({ "from": from.to_string(), "to": to.to_string(), "display_currency": currency, "movers": m })))
}

// ── Goals ──

async fn goals(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = wealth::get_settings(&state.pool).await?;
    let today = OffsetDateTime::now_utc().date();
    // Five years of month-ends: enough to measure a rate, little enough to stay fast.
    let bd = wealth::breakdown(&state.pool, &AssetFilter::default(), &settings.display_currency, 60, "month").await?;
    // Each month's organic return: revaluation of what was already held over the month's
    // opening net worth. An asset added to the book that month is not growth.
    let dates: Vec<Date> = bd.points.iter().map(|p| p.at).collect();
    let (_, valued) = valued_on(&state, &dates).await?;
    let returns: Vec<(Date, f64)> = valued
        .windows(2)
        .zip(dates.iter().skip(1))
        .filter_map(|(pair, day)| {
            let m = engine::movers::attribute(&pair[0], &pair[1]);
            (m.total_from > 0.0).then(|| {
                let revalued: f64 = m.categories.iter().map(|c| c.valuation).sum();
                (*day, revalued / m.total_from)
            })
        })
        .collect();
    let growth = engine::goals::growth_from_returns(&returns);
    let projection = engine::goals::project(bd.net_worth, settings.goal_value, growth.map(|g| g.0), today);

    // Saving over the last year: the change minus what markets and assumed drift did. Only
    // when the book is a year old, else the "change" is the book being typed in.
    let year_ago = today - time::Duration::days(365);
    let (savings, market_effect) = if growth.is_ok() {
        let (_, mut both) = valued_on(&state, &[year_ago, today]).await?;
        let end = both.pop().unwrap_or_default();
        let start = both.pop().unwrap_or_default();
        let m = engine::movers::attribute(&start, &end);
        (Some(m.total_delta - m.market_effect - m.drift_effect), Some(m.market_effect))
    } else {
        (None, None)
    };
    let fire = engine::goals::fire_number(settings.monthly_expenses);
    Ok(Json(json!({
        "display_currency": settings.display_currency,
        "net_worth": bd.net_worth,
        "goal_value": settings.goal_value,
        "goal_date": settings.goal_date.map(|d| d.to_string()),
        "monthly_expenses": settings.monthly_expenses,
        "growth_pct_year": growth.as_ref().ok().map(|g| g.0 * 100.0),
        "history_months": growth.as_ref().ok().map(|g| g.1),
        "growth_unavailable": growth.err(),
        "projection": projection,
        "on_track": match (projection.date, settings.goal_date) {
            (Some(p), Some(g)) => Some(p <= g),
            _ => None,
        },
        "savings_12m": savings,
        "market_effect_12m": market_effect,
        "savings_rate_pct": savings.and_then(|s| engine::goals::savings_rate(s, settings.monthly_expenses)),
        "fire_number": fire,
        "fire_progress_pct": fire.map(|f| bd.net_worth / f * 100.0),
    })))
}

// ── Targets and categories ──

async fn drift_report(state: &AppState) -> Result<Value, ApiError> {
    let targets = wealth::list_targets(&state.pool).await?;
    let (currency, rows) = valued_at(state, OffsetDateTime::now_utc().date()).await?;
    let report = match engine::targets::drift(&wealth::by_category(&rows), &targets) {
        Ok(r) => json!({ "ok": true, "report": r }),
        Err(reason) => json!({ "ok": false, "reason": reason }),
    };
    Ok(json!({ "display_currency": currency, "targets": targets, "drift": report }))
}

async fn get_targets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(drift_report(&state).await?))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TargetsBody {
    targets: Vec<wealth::Target>,
}

async fn put_targets(
    State(state): State<AppState>,
    Json(mut body): Json<TargetsBody>,
) -> Result<Json<Value>, ApiError> {
    for t in &mut body.targets {
        t.category = t.category.trim().to_string();
    }
    engine::targets::validate(&body.targets).map_err(|e| ApiError::bad_request(&e))?;
    wealth::replace_targets(&state.pool, &body.targets).await?;
    Ok(Json(drift_report(&state).await?))
}

/// Every category in use with its liquid flag (unflagged = illiquid).
async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let flagged = wealth::list_categories(&state.pool).await?;
    let assets = wealth::list_assets(&state.pool, &AssetFilter::default()).await?;
    let mut names: Vec<String> = assets.iter().map(|a| a.category.clone().unwrap_or_default()).collect();
    names.extend(flagged.iter().map(|c| c.name.clone()));
    names.sort();
    names.dedup();
    let rows: Vec<Value> = names
        .into_iter()
        .map(|n| {
            let liquid = flagged.iter().any(|c| c.name == n && c.liquid);
            json!({ "name": n, "liquid": liquid })
        })
        .collect();
    Ok(Json(json!({ "categories": rows })))
}

async fn put_category(
    State(state): State<AppState>,
    Json(c): Json<wealth::Category>,
) -> Result<Json<Value>, ApiError> {
    wealth::set_category_liquid(&state.pool, c.name.trim(), c.liquid).await?;
    list_categories(State(state)).await
}

// ── Real net worth inputs ──

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RealQuery {
    /// A Fundamentals series id; the saved choice, else US CPI, when omitted.
    series_id: Option<Uuid>,
}

/// The CPI series loaded in Fundamentals that can deflate the curve, and the chosen one's
/// observations. Read only: loading a series is the Fundamentals module's job.
async fn real_inputs(
    State(state): State<AppState>,
    Query(q): Query<RealQuery>,
) -> Result<Json<Value>, ApiError> {
    let settings = wealth::get_settings(&state.pool).await?;
    let all = otw_store::fundamentals::list_series(&state.pool).await?;
    // An index deflates; a rate in percent does not, so it is not offered.
    let candidates: Vec<_> = all
        .into_iter()
        .filter(|s| s.category == "inflation" && !s.unit.contains('%'))
        .collect();
    let chosen = q
        .series_id
        .or(settings.cpi_series_id)
        .and_then(|id| candidates.iter().find(|s| s.id == id))
        .or_else(|| candidates.iter().find(|s| s.provider == "fred" && s.code == "CPIAUCSL"))
        .or_else(|| candidates.first());
    let observations: Vec<(String, f64)> = match chosen {
        Some(s) => otw_store::fundamentals::observations(&state.pool, s.id, None, None)
            .await?
            .into_iter()
            .map(|(d, v)| (d.to_string(), v))
            .collect(),
        None => Vec::new(),
    };
    let series: Vec<Value> = candidates
        .iter()
        .map(|s| json!({ "id": s.id, "provider": s.provider, "code": s.code, "title": s.title, "country": s.country }))
        .collect();
    Ok(Json(json!({
        "series": series,
        "selected": chosen.map(|s| s.id),
        "observations": observations,
    })))
}

// ── Attachments ──

async fn list_attachments(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = wealth::list_attachments(&state.pool, id).await?;
    Ok(Json(json!({ "attachments": rows })))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
    revision_id: Option<Uuid>,
}

/// Raw body upload: the bytes are the file, `name` the file name, the content type its MIME.
async fn upload_attachment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<UploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    engine::check_upload(body.len(), crate::demo::enabled()).map_err(|e| ApiError::bad_request(&e))?;
    if wealth::get_asset(&state.pool, id).await?.is_none() {
        return Err(ApiError::not_found("asset not found"));
    }
    if let Some(rev) = q.revision_id {
        let revs = wealth::list_revisions(&state.pool, id).await?;
        if !revs.iter().any(|r| r.id == rev) {
            return Err(ApiError::bad_request("that revision belongs to another asset"));
        }
    }
    let name: String = q.name.trim().chars().filter(|c| !c.is_control()).take(200).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request("the file needs a name"));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|m| !m.is_empty() && m.len() < 120 && m.contains('/'))
        .unwrap_or("application/octet-stream");
    let att = wealth::add_attachment(&state.pool, id, q.revision_id, &name, mime, &body).await?;
    Ok(Json(json!({ "id": att })))
}

/// The stored bytes, always as a download: a document is never rendered in the app's origin.
async fn download_attachment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    engine::check_download(crate::demo::enabled()).map_err(|e| ApiError::forbidden(&e))?;
    let (att, bytes) = wealth::get_attachment(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("attachment not found"))?;
    let safe: String = att.name.chars().map(|c| if c == '"' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let ascii: String = safe.chars().map(|c| if c.is_ascii() { c } else { '_' }).collect();
    Ok((
        [
            (header::CONTENT_TYPE, att.mime.clone()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", urlencode(&safe)),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    ))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn delete_attachment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_attachment(&state.pool, id).await? {
        return Err(ApiError::not_found("attachment not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Export ──

async fn export_csv(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden("Exports are off in the demo."));
    }
    let assets = wealth::list_assets(&state.pool, &AssetFilter::default()).await?;
    let revisions = wealth::all_revisions(&state.pool).await?;
    let body = engine::export::csv(&assets, &revisions);
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"mywealth.csv\"".to_string()),
        ],
        body,
    ))
}
