//! HTTP API for the portfolio analytics registry, its targets, and its stress scenarios.
//!
//! One read endpoint serves every measure: `?blocks=` names which analyses to run, the
//! substrate each of them needs is built once, and a block that cannot answer says why
//! instead of failing the request. Adding an analysis adds no route here.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::portfolios::analytics::{self, substrate::Substrate, Window};
use crate::portfolios::history;
use crate::{ApiError, AppState};
use otw_store::portfolios as store;
use otw_store::portfolios_risk as risk_store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/portfolios/{id}/analytics", get(analytics_get))
        .route("/api/portfolios/{id}/targets", get(targets_get).put(targets_put))
        .route("/api/portfolios/{id}/stress", post(stress_post))
        .route("/api/portfolios/{id}/history/coverage", get(coverage_get))
        .route("/api/portfolios/{id}/history/sync", post(sync_post))
        .route("/api/portfolios/{id}/history/rebuild", post(rebuild_post))
        .route("/api/portfolios/scenarios", get(scenarios_get).post(scenario_post))
        .route("/api/portfolios/scenarios/{id}", put(scenario_put).delete(scenario_delete))
        .route("/api/portfolios/factors", get(factors_get))
        .route("/api/portfolios/blocks", get(blocks_get))
}

async fn portfolio(state: &AppState, id: Uuid) -> Result<store::Portfolio, ApiError> {
    store::get_portfolio(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))
}

fn parse_iso(s: Option<&str>) -> Result<Option<Date>, ApiError> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map(Some)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
    }
}

// ── Analytics ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AnalyticsQuery {
    /// Comma-separated block keys. Absent = every registered analysis.
    blocks: Option<String>,
    /// A named window (`ytd`, `1y`, `3y`, `inception`, …). `from`/`to` win over it.
    window: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn analytics_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Value>, ApiError> {
    let pf = portfolio(&state, id).await?;
    let (from, to) = (parse_iso(q.from.as_deref())?, parse_iso(q.to.as_deref())?);
    let window = if from.is_some() || to.is_some() {
        Window::explicit(from, to)
    } else {
        Window::named(q.window.as_deref().unwrap_or("inception"), Window::today())
    };

    let selected = analytics::selected(q.blocks.as_deref());
    let needs = analytics::needs_of(&selected);
    let s = Substrate::build(&state.pool, &pf, window, needs, None).await?;
    Ok(Json(json!({
        "portfolio": pf,
        "window": { "label": window.label, "from": window.from.map(|d| d.to_string()), "to": window.to.map(|d| d.to_string()) },
        "blocks": analytics::run(&selected, &s),
    })))
}

/// The registered blocks and what each needs, so a client can build its tab bar from the
/// server rather than from a hardcoded list that drifts.
async fn blocks_get() -> Json<Value> {
    Json(json!({
        "blocks": analytics::ANALYSES.iter().map(|a| json!({ "key": a.key() })).collect::<Vec<_>>(),
        "windows": analytics::WINDOWS,
    }))
}

async fn factors_get() -> Json<Value> {
    Json(json!({
        "factors": analytics::substrate::FACTOR_DEFS.iter().map(|f| json!({
            "id": f.id,
            "proxies": f.proxies,
            "in_bps": f.in_bps,
        })).collect::<Vec<_>>(),
    }))
}

// ── Targets ───────────────────────────────────────────────────────────────────

async fn targets_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "targets": risk_store::list_targets(&state.pool, id).await? })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TargetsBody {
    /// `asset_class` today. Anything else is refused rather than stored and ignored.
    #[serde(default = "asset_class")]
    dimension: String,
    rows: Vec<TargetRow>,
}

fn asset_class() -> String {
    "asset_class".into()
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TargetRow {
    bucket: String,
    target_pct: f64,
    #[serde(default = "default_band")]
    band_pct: f64,
}

fn default_band() -> f64 {
    5.0
}

/// Replace one dimension's targets. Wholesale, because a target allocation is a single
/// statement that has to add up: patching bucket by bucket would let the set sit invalid.
async fn targets_put(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<TargetsBody>,
) -> Result<Json<Value>, ApiError> {
    portfolio(&state, id).await?;
    if b.dimension != "asset_class" {
        return Err(ApiError::bad_request("only asset_class targets are supported"));
    }
    let mut rows = Vec::with_capacity(b.rows.len());
    let mut total = 0.0;
    for r in &b.rows {
        let bucket = r.bucket.trim();
        if bucket.is_empty() {
            return Err(ApiError::bad_request("a target needs a bucket"));
        }
        // `cash` is a bucket the ledger produces, not an asset class an asset can carry.
        if bucket != "cash" && !crate::portfolios_api::ASSET_CLASSES.contains(&bucket) {
            return Err(ApiError::bad_request(&format!("unknown bucket: {bucket}")));
        }
        if !(0.0..=100.0).contains(&r.target_pct) {
            return Err(ApiError::bad_request("a target is between 0 and 100 percent"));
        }
        if r.band_pct < 0.0 {
            return Err(ApiError::bad_request("a band cannot be negative"));
        }
        total += r.target_pct;
        rows.push(risk_store::Target {
            dimension: b.dimension.clone(),
            bucket: bucket.to_string(),
            target_pct: r.target_pct,
            band_pct: r.band_pct,
        });
    }
    // An allocation that does not add up to the whole book is not an allocation. Empty
    // clears the dimension, which is how the drift table is turned off.
    if !rows.is_empty() && (total - 100.0).abs() > 0.01 {
        return Err(ApiError::bad_request(&format!(
            "targets must add up to 100% (they add up to {total:.2}%)"
        )));
    }
    risk_store::set_targets(&state.pool, id, &b.dimension, &rows).await?;
    Ok(Json(json!({ "targets": rows })))
}

// ── Scenarios ─────────────────────────────────────────────────────────────────

async fn scenarios_get(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "scenarios": risk_store::list_scenarios(&state.pool).await? })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ScenarioBody {
    name: String,
    /// `factor` (a set of shocks) or `historical` (a window to replay).
    kind: String,
    /// The shocks, or the window: the shape follows `kind`.
    #[schemars(with = "ScenarioLegs")]
    legs: Value,
    #[serde(default)]
    note: String,
}

/// What `legs` holds, published rather than left as "any JSON": a caller told only that a
/// scenario has legs invents a `shock` field, which is a 400 it cannot debug from here.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(crate) enum ScenarioLegs {
    /// kind=factor: one entry per shocked factor (factor ids come from GET /api/portfolios/factors).
    Factor(Vec<crate::portfolios::analytics::stress::Leg>),
    /// kind=historical: the window to replay.
    Historical(HistoricalWindow),
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct HistoricalWindow {
    /// YYYY-MM-DD, inclusive.
    from: String,
    /// YYYY-MM-DD, after `from`.
    to: String,
}

fn validate_scenario(b: &ScenarioBody) -> Result<(), ApiError> {
    match b.kind.as_str() {
        "factor" => {
            let legs: Vec<crate::portfolios::analytics::stress::Leg> =
                serde_json::from_value(b.legs.clone())
                    .map_err(|_| ApiError::bad_request("legs must be a list of factor shocks"))?;
            if legs.is_empty() {
                return Err(ApiError::bad_request("a factor scenario needs at least one leg"));
            }
            crate::portfolios::analytics::stress::validate_legs(&legs).map_err(|e| ApiError::bad_request(&e))
        }
        "historical" => {
            let from = b.legs.get("from").and_then(|v| v.as_str());
            let to = b.legs.get("to").and_then(|v| v.as_str());
            let (Some(from), Some(to)) = (from, to) else {
                return Err(ApiError::bad_request("a historical scenario needs a from and a to"));
            };
            parse_iso(Some(from))?;
            parse_iso(Some(to))?;
            if from >= to {
                return Err(ApiError::bad_request("the window ends before it starts"));
            }
            Ok(())
        }
        _ => Err(ApiError::bad_request("kind must be 'factor' or 'historical'")),
    }
}

async fn scenario_post(
    State(state): State<AppState>,
    Json(b): Json<ScenarioBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    validate_scenario(&b)?;
    let s = risk_store::upsert_scenario(&state.pool, None, b.name.trim(), &b.kind, &b.legs, b.note.trim()).await?;
    Ok(Json(json!({ "scenario": s })))
}

async fn scenario_put(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ScenarioBody>,
) -> Result<Json<Value>, ApiError> {
    validate_scenario(&b)?;
    let s = risk_store::upsert_scenario(&state.pool, Some(id), b.name.trim(), &b.kind, &b.legs, b.note.trim()).await?;
    Ok(Json(json!({ "scenario": s })))
}

async fn scenario_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !risk_store::delete_scenario(&state.pool, id).await? {
        // A builtin would come back on the next migration, and a row that reappears after a
        // delete is worse than one that never left.
        return Err(ApiError::bad_request("a shipped scenario cannot be deleted, only edited"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Running a stress ──────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StressBody {
    /// A saved scenario to run. Alternatively pass `legs` (factor) or `from`/`to` (replay).
    #[serde(default)]
    scenario_id: Option<Uuid>,
    /// An ad-hoc factor shock, same shape as a saved scenario's factor legs.
    #[serde(default)]
    #[schemars(with = "Option<Vec<crate::portfolios::analytics::stress::Leg>>")]
    legs: Option<Value>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

/// Shock a book. Writes nothing.
async fn stress_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<StressBody>,
) -> Result<Json<Value>, ApiError> {
    let pf = portfolio(&state, id).await?;

    // Resolve what to run before touching any data: a replay reaches back to its own window
    // and the substrate has to be told how deep to read.
    let (kind, legs, from, to, name) = match b.scenario_id {
        Some(sid) => {
            let s = risk_store::get_scenario(&state.pool, sid)
                .await?
                .ok_or_else(|| ApiError::not_found("scenario not found"))?;
            match s.kind.as_str() {
                "historical" => (
                    "historical",
                    None,
                    s.legs.get("from").and_then(|v| v.as_str()).map(str::to_string),
                    s.legs.get("to").and_then(|v| v.as_str()).map(str::to_string),
                    s.name,
                ),
                _ => ("factor", Some(s.legs.clone()), None, None, s.name),
            }
        }
        None if b.legs.is_some() => ("factor", b.legs.clone(), None, None, "Custom shock".to_string()),
        None if b.from.is_some() && b.to.is_some() => {
            ("historical", None, b.from.clone(), b.to.clone(), "Custom replay".to_string())
        }
        None => return Err(ApiError::bad_request("pass a scenario_id, legs, or a from/to window")),
    };

    let replay_start = parse_iso(from.as_deref())?;
    let needs = analytics::Needs::BOOK | analytics::Needs::MARKS | analytics::Needs::FACTORS;
    let s = Substrate::build(&state.pool, &pf, Window::inception(), needs, replay_start).await?;
    let (Some(book), Some(marks), Some(factors)) = (s.book.as_ref(), s.marks.as_ref(), s.factors.as_ref())
    else {
        return Err(ApiError::bad_request("the book could not be read"));
    };
    // Same refusal as the block: an impact quoted as a share of negative equity is a share of
    // capital that is not there.
    if book.net_worth < 0.0 {
        return Err(ApiError::bad_request("this book has negative equity: there is no base to shock"));
    }

    let impact = match kind {
        "historical" => {
            let (Some(from), Some(to)) = (from.as_deref(), to.as_deref()) else {
                return Err(ApiError::bad_request("a replay needs a from and a to"));
            };
            crate::portfolios::analytics::stress::run_historical(book, marks, from, to)
        }
        _ => {
            let legs: Vec<crate::portfolios::analytics::stress::Leg> =
                serde_json::from_value(legs.unwrap_or(json!([])))
                    .map_err(|_| ApiError::bad_request("legs must be a list of factor shocks"))?;
            crate::portfolios::analytics::stress::validate_legs(&legs).map_err(|e| ApiError::bad_request(&e))?;
            crate::portfolios::analytics::stress::run_factor(book, marks, factors, &legs)
        }
    };

    Ok(Json(json!({
        "scenario": { "name": name, "kind": kind, "from": from, "to": to },
        "currency": book.currency,
        "net_worth": book.net_worth,
        "impact": impact,
        // Every position the model could not reach, named. The headline above is only true
        // of the share this list does not cover.
        "uncovered": marks.uncovered,
    })))
}

// ── History ───────────────────────────────────────────────────────────────────

async fn coverage_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let pf = portfolio(&state, id).await?;
    Ok(Json(json!({ "coverage": history::coverage(&state.pool, &pf).await? })))
}

async fn sync_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let pf = portfolio(&state, id).await?;
    Ok(Json(json!({ "sync": history::sync(&state.pool, &pf).await? })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RebuildBody {
    /// Rebuild from this day forward. Absent uses the portfolio's own staleness watermark,
    /// and failing that the first operation.
    #[serde(default)]
    from: Option<String>,
}

async fn rebuild_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<RebuildBody>,
) -> Result<Json<Value>, ApiError> {
    let pf = portfolio(&state, id).await?;
    let from = parse_iso(b.from.as_deref())?.or(pf.rebuild_from);
    Ok(Json(json!({ "rebuild": history::rebuild(&state.pool, &pf, from).await? })))
}
