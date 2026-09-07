//! HTTP API for the backtest optimizer (parameter grid as a job).
//!
//! - `POST   /api/backtest/optimize/params`    the parameters this strategy can vary
//! - `POST   /api/backtest/optimize/estimate`  count the variants and time one of them
//! - `POST   /api/backtest/optimize`           start the grid, return its job id
//! - `GET    /api/backtest/optimize`           the jobs held in memory
//! - `GET    /api/backtest/optimize/{id}`      progress + one page of the ranking
//!   (`?sort=&dir=&offset=&limit=&analysis=`)
//! - `POST   /api/backtest/optimize/{id}/cancel`
//! - `DELETE /api/backtest/optimize/{id}`
//!
//! The estimate is a *measurement*, not a model: it loads the bars and runs the first variant,
//! so the count the user is asked to confirm ("130,293 variants, about 43 minutes") comes from
//! this machine and this strategy rather than from a table of guesses.
//!
//! A job lives in memory only: a sweep is an exploration, and the run history stays the place
//! where a result is kept. Its `settings` + `axes` are echoed back on read, so the results page
//! needs nothing but the job id to rebuild itself after a reload.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::backtest::optimize::{self, Axis};
use crate::backtest::Settings;
use crate::backtest_api::{load_assets, resolve_window};
use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/optimize", post(start).get(list))
        .route("/api/backtest/optimize/params", post(params))
        .route("/api/backtest/optimize/estimate", post(estimate))
        .route("/api/backtest/optimize/{id}", get(status).delete(forget))
        .route("/api/backtest/optimize/{id}/cancel", post(cancel))
}

/// Rows per ranking page. The table paginates; the whole grid is never serialized.
const MAX_PAGE: usize = 500;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GridBody {
    /// Single dataset to simulate. Optional when `dataset_ids` is given.
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Portfolio dataset set (up to 8). Falls back to `[dataset_id]` when absent.
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    /// Base settings; every variant is this with one combination patched in. Same shape as
    /// `/api/backtest/run`; get a working example from `GET /api/backtest/strategies/{id}`.
    ///
    /// Raw JSON so an axis can patch a path the typed struct would have normalized away,
    /// advertised as the engine `Settings` so the published schema is the real one.
    #[schemars(with = "Settings")]
    settings: Value,
    /// The parameters to vary. Get the paths that exist from `/api/backtest/optimize/params`.
    /// Values are written into the settings verbatim, so they are in engine units.
    #[serde(default)]
    axes: Vec<Axis>,
    /// Cap on bars loaded per dataset. Applied to the most recent bars of the window.
    #[serde(default)]
    limit: Option<i64>,
    /// Start of the simulated window, inclusive. "YYYY-MM-DD" or RFC3339.
    #[serde(default)]
    from: Option<String>,
    /// End of the simulated window, inclusive. A plain date covers that whole day.
    #[serde(default)]
    to: Option<String>,
    /// Metric the ranking opens on. Any listed metric; the table can re-sort on the others.
    #[serde(default)]
    metric: Option<String>,
}

impl GridBody {
    fn ids(&self) -> Result<Vec<Uuid>, ApiError> {
        let ids = if !self.dataset_ids.is_empty() {
            self.dataset_ids.clone()
        } else if let Some(id) = self.dataset_id {
            vec![id]
        } else {
            return Err(ApiError::bad_request("no dataset selected"));
        };
        if ids.len() > 8 {
            return Err(ApiError::bad_request("at most 8 datasets per portfolio run"));
        }
        Ok(ids)
    }

    fn metric(&self) -> String {
        match self.metric.as_deref() {
            Some(m) if optimize::is_metric(m) => m.to_string(),
            _ => "sharpe".to_string(),
        }
    }
}

/// The optimizer is the CPU-heaviest thing in the app, and a shared demo host has no business
/// running one, and no per-IP budget makes that different.
fn allowed() -> Result<(), ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::bad_request(
            "the optimizer is disabled in the demo: a parameter grid takes every core for minutes",
        ));
    }
    Ok(())
}

/// Ask a strategy which of its parameters a grid can vary.
///
/// The screen builds this list client-side from the indicator catalog it already has. A caller
/// that is not the screen (an agent over MCP) would otherwise have to spell settings paths from
/// memory, and a path that does not exist is not an error: it is written into the settings as a
/// dead key, and the grid then runs the same strategy N times without saying so.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ParamsBody {
    /// Read the settings from a saved strategy. Either this or `settings`.
    #[serde(default)]
    pub strategy_id: Option<Uuid>,
    /// Settings to inspect, same shape as `/api/backtest/run`. Wins over `strategy_id`.
    ///
    /// Held as raw JSON because discovery reports the paths the caller's own object has, but
    /// advertised as the engine `Settings` so the schema is the real one and not "any JSON".
    #[serde(default)]
    #[schemars(with = "Option<Settings>")]
    pub settings: Option<Value>,
}

async fn params(
    State(state): State<AppState>,
    Json(body): Json<ParamsBody>,
) -> Result<Json<Value>, ApiError> {
    let settings = match (body.settings, body.strategy_id) {
        (Some(s), _) => s,
        (None, Some(id)) => {
            otw_store::backtest::get_strategy(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("strategy not found"))?
                .settings
        }
        (None, None) => return Err(ApiError::bad_request("settings or strategy_id is required")),
    };
    // Deserialize before reporting: paths are only worth anything if the engine accepts the
    // object they index.
    let typed: Settings = serde_json::from_value(settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("settings are not runnable: {e}")))?;
    if let Some(err) = typed.validate() {
        return Err(ApiError::bad_request(&err));
    }
    let params = crate::backtest::params::discover(&settings);
    Ok(Json(json!({
        "params": params,
        "max_axes": optimize::MAX_AXES,
        "max_axis_values": optimize::MAX_AXIS_VALUES,
        "max_trials": optimize::MAX_TRIALS,
    })))
}

/// Check the grid, load the bars, and time variant 0. Writes nothing and starts nothing.
async fn estimate(
    State(state): State<AppState>,
    Json(body): Json<GridBody>,
) -> Result<Json<Value>, ApiError> {
    allowed()?;
    let ids = body.ids()?;
    let total = optimize::validate(&body.axes).map_err(|e| ApiError::bad_request(&e))?;
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    let assets = load_assets(&state, &ids, limit, from, to).await?;
    let bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    points_budget(total, bars)?;

    // One real variant, timed. Blocking work off the runtime; the first variant also proves the
    // grid patches into something the engine accepts, which is what turns a bad path into a 400
    // here instead of 130k identical failures later.
    let axes = body.axes.clone();
    let base = body.settings.clone();
    let (per_trial_ms, warmup_bars) = tokio::task::spawn_blocking(move || {
        let refs: Vec<_> = assets.iter().map(|a| a.as_bars()).collect();
        let refs: Vec<_> = refs.iter().collect();
        let patched = optimize::patched(&base, &axes, 0)?;
        let settings: Settings = serde_json::from_value(patched)
            .map_err(|e| format!("the first variant is not runnable: {e}"))?;
        if let Some(err) = settings.validate() {
            return Err(err);
        }
        let t0 = std::time::Instant::now();
        let res = crate::backtest::run_portfolio(&settings, &refs);
        Ok((t0.elapsed().as_secs_f64() * 1000.0, res.warmup_bars))
    })
    .await
    .map_err(|e| ApiError::internal(&format!("estimate failed: {e}")))?
    .map_err(|e: String| ApiError::bad_request(&e))?;

    let workers = optimize::worker_count();
    // Threads never scale perfectly (memory bandwidth, the shared row lock): 0.85 keeps the
    // headline number on the honest side of what the job will really do.
    let est_ms = (total as f64 * per_trial_ms / (workers as f64 * 0.85)).round();
    Ok(Json(json!({
        "trials": total,
        "bars": bars,
        "warmup_bars": warmup_bars,
        "per_trial_ms": per_trial_ms,
        "workers": workers,
        "est_ms": est_ms,
        "running": state.optimizer.running().map(|j| j.id),
    })))
}

/// Refuse a grid whose simulated work is beyond any reasonable wait, whatever the trial count.
fn points_budget(total: u64, bars: usize) -> Result<(), ApiError> {
    let points = total.saturating_mul(bars as u64);
    if points > optimize::MAX_POINTS {
        return Err(ApiError::bad_request(&format!(
            "{total} variants over {bars} bars is {points} simulated points, past the \
             {} budget. Narrow the date range, or drop a parameter",
            optimize::MAX_POINTS
        )));
    }
    Ok(())
}

/// Start the grid. Returns as soon as the job exists; progress is polled.
async fn start(
    State(state): State<AppState>,
    Json(body): Json<GridBody>,
) -> Result<Json<Value>, ApiError> {
    allowed()?;
    let ids = body.ids()?;
    let total = optimize::validate(&body.axes).map_err(|e| ApiError::bad_request(&e))?;
    if let Some(job) = state.optimizer.running() {
        return Err(ApiError::bad_request(&format!(
            "an optimization is already running ({} of {} variants). Stop it first",
            job.progress()["done"],
            job.total
        )));
    }
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    let assets = load_assets(&state, &ids, limit, from, to).await?;
    let bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    points_budget(total, bars)?;

    // Variant 0 is validated before anything is spawned: a grid pointing at a field that does
    // not exist must fail as a request, not as a job that immediately dies.
    let first = optimize::patched(&body.settings, &body.axes, 0)
        .map_err(|e| ApiError::bad_request(&e))?;
    let settings: Settings = serde_json::from_value(first)
        .map_err(|e| ApiError::bad_request(&format!("the first variant is not runnable: {e}")))?;
    if let Some(err) = settings.validate() {
        return Err(ApiError::bad_request(&err));
    }

    let job = state.optimizer.add(
        optimize::NewJob {
            dataset_ids: ids,
            ticker: assets[0].ticker.clone(),
            timeframe: assets[0].timeframe.clone(),
            bars,
            from,
            to,
            base: body.settings.clone(),
            axes: body.axes.clone(),
            metric: body.metric(),
        },
        total,
    );

    // The bars move into the blocking task and are dropped with it; the job itself outlives the
    // work so its rows stay readable afterwards.
    let handle = job.clone();
    tokio::task::spawn_blocking(move || optimize::work(&handle, &assets));

    Ok(Json(json!({
        "job_id": job.id,
        "total": total,
        "workers": job.workers,
        "bars": bars,
    })))
}

#[derive(Deserialize)]
struct StatusQuery {
    /// Metric to sort the ranking on. Defaults to the job's own.
    #[serde(default)]
    sort: Option<String>,
    /// "asc" flips the natural direction of that metric.
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    /// Include the per-parameter effect table and the metric distribution. Off by default: the
    /// poll that only advances a progress bar has no business scanning 250k rows.
    #[serde(default)]
    analysis: Option<bool>,
}

impl StatusQuery {
    fn sort_on(&self, job: &optimize::Job) -> (String, bool) {
        let metric = match self.sort.as_deref() {
            Some(m) if optimize::is_metric(m) => m.to_string(),
            _ => job.metric.clone(),
        };
        let desc = match self.dir.as_deref() {
            Some("asc") => false,
            Some("desc") => true,
            _ => optimize::higher_better(&metric),
        };
        (metric, desc)
    }
}

fn job_or_404(state: &AppState, id: Uuid) -> Result<Arc<optimize::Job>, ApiError> {
    state
        .optimizer
        .get(id)
        .ok_or_else(|| ApiError::not_found("that optimization is no longer in memory"))
}

/// Progress + one page of the ranking. Called on a timer while the job runs, and once after.
async fn status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let job = job_or_404(&state, id)?;
    let (metric, desc) = q.sort_on(&job);
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(50).clamp(1, MAX_PAGE);
    let mut out = json!({
        "job": job.head(),
        "progress": job.progress(),
        "sort": metric,
        "dir": if desc { "desc" } else { "asc" },
        "rows": job.ranking(&metric, desc, offset, limit),
        "row_count": job.row_count(),
    });
    if q.analysis.unwrap_or(false) {
        let (dist, sharpes) = job.distribution(&metric, 24);
        // The haircut the trial count earns: the best Sharpe of N tries is not the Sharpe of a
        // strategy, and this is the number that says by how much.
        let deflated = crate::quant::deflated_sharpe(
            &sharpes,
            job.bars,
            crate::quant::periods_per_year(&job.timeframe),
        );
        out["sensitivity"] = job.sensitivity(&metric);
        out["distribution"] = dist;
        out["deflated_sharpe"] = serde_json::to_value(deflated).unwrap_or(Value::Null);
    }
    Ok(Json(out))
}

/// Stop the workers. What already finished stays readable, which is the whole point of stopping
/// rather than deleting.
async fn cancel(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let job = job_or_404(&state, id)?;
    job.cancel();
    Ok(Json(json!({ "ok": true, "progress": job.progress() })))
}

async fn forget(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !state.optimizer.remove(id) {
        return Err(ApiError::not_found("that optimization is no longer in memory"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// The jobs still in memory, newest first. Lets the module offer its way back to a sweep that
/// is still running when the page was left.
async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jobs: Vec<Value> = state
        .optimizer
        .list()
        .iter()
        .map(|j| {
            let mut head = j.head();
            head["progress"] = j.progress();
            head["total"] = json!(j.total);
            head
        })
        .collect();
    Ok(Json(json!({ "jobs": jobs, "metrics": optimize::METRICS.iter().map(|(id, hb)| json!({ "id": id, "higher_better": hb })).collect::<Vec<_>>() })))
}
