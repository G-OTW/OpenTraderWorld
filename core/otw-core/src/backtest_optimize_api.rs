//! HTTP API for the backtest optimizer (parameter grid as a job).
//!
//! - `POST   /api/backtest/optimize/params`    the parameters this strategy can vary
//! - `POST   /api/backtest/optimize/estimate`  count the variants and time one of them
//! - `POST   /api/backtest/optimize`           start the grid, return its job id
//! - `GET    /api/backtest/optimize`           the jobs held in memory
//! - `GET    /api/backtest/optimize/{id}`      progress + one page of the ranking
//!   (`?sort=&dir=&offset=&limit=&analysis=`)
//! - `GET    /api/backtest/optimize/{id}/cloud` three dimensions of every trial, filtered and thinned
//!   (`?x=&y=&z=&filter=&max=&sort=&dir=`)
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
use crate::backtest_api::{check_single_asset, execution_of, load_run_assets, resolve_window, settings_kind};
use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/optimize", post(start).get(list))
        .route("/api/backtest/optimize/params", post(params))
        .route("/api/backtest/optimize/estimate", post(estimate))
        .route("/api/backtest/optimize/{id}", get(status).delete(forget))
        .route("/api/backtest/optimize/{id}/cloud", get(cloud))
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

    fn metric(&self) -> Result<String, ApiError> {
        match self.metric.as_deref() {
            None => Ok("sharpe".to_string()),
            Some(m) => parse_metric(m, "metric"),
        }
    }
}

/// A ranking metric by name, or a 400 listing the ones there are. An unknown name used to fall
/// back to Sharpe (or the job's metric) without a word, so the caller read a ranking it never
/// asked for.
fn parse_metric(m: &str, field: &str) -> Result<String, ApiError> {
    let key = m.trim().to_ascii_lowercase();
    if optimize::is_metric(&key) {
        return Ok(key);
    }
    let valid: Vec<&str> = optimize::METRICS.iter().map(|(n, _)| *n).collect();
    Err(ApiError::bad_request(&format!("unknown {field} \"{m}\". Valid: {}", valid.join(", "))))
}

/// Query flags written as true/false, 1/0 or yes/no (a URL has no booleans, and clients differ).
// TODO: switch to util::lenient_bool
fn lenient_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let raw: Option<String> = Option::deserialize(d)?;
    let Some(raw) = raw else { return Ok(None) };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(None),
        "true" | "1" | "yes" | "on" => Ok(Some(true)),
        "false" | "0" | "no" | "off" => Ok(Some(false)),
        _ => Err(serde::de::Error::custom(format!("expected true/false, 1/0 or yes/no (got \"{raw}\")"))),
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
/// memory. A path the engine does not read is refused as a `dead path`, so read the paths here.
/// A length shared by several operands (the four MACD operands of a cross) is one parameter
/// listing every path: put all of them in one axis's `paths`.
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
    // Script inputs are parameters too: their names, ranges and steps live in the compiled
    // metadata (`validate` has already refused a script that does not compile).
    let mut metas = std::collections::BTreeMap::new();
    for (id, def) in &typed.scripts {
        if let Ok(c) = crate::script::compile(def.source.clone()).await {
            metas.insert(id.clone(), c.meta);
        }
    }
    let params = crate::backtest::params::discover(&settings, &metas);
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
    body.metric()?;
    check_single_asset(settings_kind(&body.settings), &ids)?;
    let total = optimize::validate(&body.settings, &body.axes).map_err(|e| ApiError::bad_request(&e))?;
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    let scripts = crate::backtest::script::used_json(&body.settings);
    let (assets, data_warnings, _) = load_run_assets(&state, &ids, limit, from, to, &execution_of(&body.settings), None, scripts).await?;
    let bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    points_budget(total, bars)?;

    // One real variant, timed. Blocking work off the runtime; the first variant also proves the
    // grid patches into something the engine accepts, which is what turns a bad path into a 400
    // here instead of 130k identical failures later.
    let axes = body.axes.clone();
    let base = body.settings.clone();
    let (run_ms, script_ms, warmup_bars, mut warnings) = tokio::task::spawn_blocking(move || {
        let refs: Vec<_> = assets.iter().map(|a| a.as_bars()).collect();
        let refs: Vec<_> = refs.iter().collect();
        let patched = optimize::patched(&base, &axes, 0)?;
        let settings: Settings = serde_json::from_value(patched)
            .map_err(|e| format!("the first variant is not runnable: {e}"))?;
        if let Some(err) = settings.validate() {
            return Err(err);
        }
        // A script run is timed on its own: trials sharing its input values reuse it.
        let t0 = std::time::Instant::now();
        crate::backtest::script::prepare(&settings, &refs)?;
        let script_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        let res = crate::backtest::run_portfolio(&settings, &refs);
        let run_ms = t1.elapsed().as_secs_f64() * 1000.0;
        Ok((run_ms, script_ms, res.warmup_bars, settings.warnings()))
    })
    .await
    .map_err(|e| ApiError::internal(&format!("estimate failed: {e}")))?
    .map_err(|e: String| ApiError::bad_request(&e))?;

    let workers = optimize::worker_count();
    // Threads never scale perfectly (memory bandwidth, the shared row lock): 0.85 keeps the
    // headline number on the honest side of what the job will really do.
    let work_ms = total as f64 * run_ms + optimize::script_runs(&body.axes, total) as f64 * script_ms;
    let per_trial_ms = work_ms / total.max(1) as f64;
    let est_ms = (work_ms / (workers as f64 * 0.85)).round();
    warnings.extend(data_warnings);
    Ok(Json(json!({
        "trials": total,
        "bars": bars,
        "warnings": warnings,
        "warmup_bars": warmup_bars,
        "per_trial_ms": per_trial_ms,
        "workers": workers,
        "est_ms": est_ms,
        "running": state.optimizer.running().map(|j| j.id),
    })))
}

/// Warnings of each job held in memory: the data's, and the settings warnings of its first
/// variant. Kept here, next to the job registry, and dropped with the job.
static JOB_WARNINGS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<Uuid, Vec<String>>>> =
    std::sync::OnceLock::new();

fn job_warnings() -> &'static std::sync::Mutex<std::collections::HashMap<Uuid, Vec<String>>> {
    JOB_WARNINGS.get_or_init(Default::default)
}

/// Record a new job's warnings, forgetting those of jobs no longer in memory.
fn keep_warnings(state: &AppState, id: Uuid, warnings: Vec<String>) {
    let mut map = job_warnings().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|k, _| state.optimizer.get(*k).is_some());
    map.insert(id, warnings);
}

fn warnings_of(id: Uuid) -> Vec<String> {
    job_warnings().lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().unwrap_or_default()
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
    let metric = body.metric()?;
    check_single_asset(settings_kind(&body.settings), &ids)?;
    let total = optimize::validate(&body.settings, &body.axes).map_err(|e| ApiError::bad_request(&e))?;
    if let Some(job) = state.optimizer.running() {
        return Err(ApiError::bad_request(&format!(
            "an optimization is already running ({} of {} variants). Stop it first",
            job.progress()["done"],
            job.total
        )));
    }
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    let scripts = crate::backtest::script::used_json(&body.settings);
    let (assets, data_warnings, _) = load_run_assets(&state, &ids, limit, from, to, &execution_of(&body.settings), None, scripts).await?;
    let bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    points_budget(total, bars)?;
    // The clock the trials' Sharpe is measured on, and how much of it the ranked figure covers.
    let (rows, ppy) = {
        let views: Vec<crate::backtest::Bars> = assets.iter().map(|a| a.as_bars()).collect();
        let refs: Vec<&crate::backtest::Bars> = views.iter().collect();
        crate::backtest::clock_rate(&refs)
    };

    // Variant 0 is validated before anything is spawned: a grid pointing at a field that does
    // not exist must fail as a request, not as a job that immediately dies.
    let first = optimize::patched(&body.settings, &body.axes, 0)
        .map_err(|e| ApiError::bad_request(&e))?;
    let settings: Settings = serde_json::from_value(first)
        .map_err(|e| ApiError::bad_request(&format!("the first variant is not runnable: {e}")))?;
    if let Some(err) = settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    {
        let views: Vec<crate::backtest::Bars> = assets.iter().map(|a| a.as_bars()).collect();
        let refs: Vec<&crate::backtest::Bars> = views.iter().collect();
        crate::backtest_api::prepare_scripts(&settings, &refs)?;
    }

    let job = state.optimizer.add(
        optimize::NewJob {
            dataset_ids: ids,
            ticker: assets[0].ticker.clone(),
            timeframe: assets[0].timeframe.clone(),
            bars,
            ppy,
            observations: in_sample_rows(rows, &settings),
            from,
            to,
            base: body.settings.clone(),
            axes: body.axes.clone(),
            metric,
        },
        total,
    );

    // The bars move into the blocking task and are dropped with it; the job itself outlives the
    // work so its rows stay readable afterwards.
    let mut warnings = settings.warnings();
    warnings.extend(data_warnings);
    keep_warnings(&state, job.id, warnings.clone());
    let handle = job.clone();
    tokio::task::spawn_blocking(move || {
        optimize::work(&handle, &assets);
        // An axis whose every value gave the same stats is config this strategy does not read.
        let flat = handle.flat_axis_warnings();
        if !flat.is_empty() {
            let mut map = job_warnings().lock().unwrap_or_else(|e| e.into_inner());
            map.entry(handle.id).or_default().extend(flat);
        }
    });

    Ok(Json(json!({
        "job_id": job.id,
        "total": total,
        "workers": job.workers,
        "bars": bars,
        "warnings": warnings,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StatusQuery {
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
    /// true/false, 1/0 or yes/no.
    #[serde(default, deserialize_with = "lenient_bool")]
    analysis: Option<bool>,
}

impl StatusQuery {
    fn sort_on(&self, job: &optimize::Job) -> Result<(String, bool), ApiError> {
        let metric = match self.sort.as_deref() {
            Some(m) if !m.trim().is_empty() => parse_metric(m, "sort")?,
            _ => job.metric.clone(),
        };
        let desc = match self.dir.as_deref() {
            Some("asc") => false,
            Some("desc") => true,
            _ => optimize::higher_better(&metric),
        };
        Ok((metric, desc))
    }
}

/// Rows the ranked Sharpe is computed over: the in-sample head when the settings split.
pub(crate) fn in_sample_rows(rows: usize, s: &Settings) -> usize {
    let split = s.oos_split_pct;
    if split > 0.0 && split < 1.0 {
        ((rows as f64) * split).round() as usize
    } else {
        rows
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
    let (metric, desc) = q.sort_on(&job)?;
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(50).clamp(1, MAX_PAGE);
    let mut out = json!({
        "job": job.head(),
        "progress": job.progress(),
        "sort": metric,
        "dir": if desc { "desc" } else { "asc" },
        "rows": job.ranking(&metric, desc, offset, limit),
        "row_count": job.row_count(),
        "warnings": warnings_of(job.id),
    });
    if q.analysis.unwrap_or(false) {
        let (dist, sharpes) = job.distribution(&metric, 24);
        // The haircut the trial count earns: the best Sharpe of N tries is not the Sharpe of a
        // strategy, and this is the number that says by how much.
        let deflated = crate::quant::deflated_sharpe(
            &sharpes,
            job.observations,
            job.ppy.unwrap_or_else(|| crate::quant::periods_per_year(&job.timeframe)),
            None,
        );
        out["sensitivity"] = job.sensitivity(&metric);
        out["distribution"] = dist;
        out["deflated_sharpe"] = serde_json::to_value(deflated).unwrap_or(Value::Null);
    }
    Ok(Json(out))
}

/// Points the 3D cloud may ask for. Past a few tens of thousands the browser, not the server, is
/// what gives out; the client asks for less on a weaker machine.
const MAX_CLOUD: usize = 50_000;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CloudQuery {
    /// Dimension on each axis: a metric or `p<k>` (the k-th swept parameter).
    x: String,
    y: String,
    z: String,
    /// `id:min:max` entries joined by `,`. An id is a metric or `p<k>` (the k-th swept parameter).
    #[serde(default)]
    filter: Option<String>,
    /// Most points returned (default 20000, clamped to 100..50000).
    #[serde(default)]
    max: Option<usize>,
    /// Metric (and direction) whose best trials survive the thinning. Defaults to the job's own.
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    dir: Option<String>,
}

/// Every finished trial on three dimensions, filtered by min/max and thinned to `max` points.
async fn cloud(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<CloudQuery>,
) -> Result<Json<Value>, ApiError> {
    let job = job_or_404(&state, id)?;
    let dim = |s: &str| {
        job.cloud_dim(s)
            .ok_or_else(|| ApiError::bad_request(&format!("unknown dimension \"{s}\": a metric or p0..p{}", job.axes.len().saturating_sub(1))))
    };
    let dims = [dim(&q.x)?, dim(&q.y)?, dim(&q.z)?];
    let mut filters = Vec::new();
    for part in q.filter.as_deref().unwrap_or("").split(',').filter(|p| !p.trim().is_empty()) {
        let mut it = part.splitn(3, ':');
        let (Some(d), Some(lo), Some(hi)) = (it.next(), it.next(), it.next()) else {
            return Err(ApiError::bad_request(&format!("filter \"{part}\": expected id:min:max")));
        };
        let num = |s: &str| {
            s.trim()
                .parse::<f32>()
                .map_err(|_| ApiError::bad_request(&format!("filter \"{part}\": \"{s}\" is not a number")))
        };
        filters.push((dim(d)?, num(lo)?, num(hi)?));
    }
    let status = StatusQuery { sort: q.sort, dir: q.dir, offset: None, limit: None, analysis: None };
    let (metric, desc) = status.sort_on(&job)?;
    let max = q.max.unwrap_or(20_000).clamp(100, MAX_CLOUD);
    Ok(Json(job.cloud(&dims, &filters, max, (&metric, desc))))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn query(q: &str) -> Result<StatusQuery, String> {
        let uri: axum::http::Uri = format!("http://x/api/backtest/optimize/1?{q}").parse().unwrap();
        Query::<StatusQuery>::try_from_uri(&uri).map(|q| q.0).map_err(|e| e.body_text())
    }

    #[test]
    fn an_unknown_metric_is_refused_with_the_list() {
        let e = parse_metric("foo", "sort").unwrap_err();
        assert_eq!(e.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(e.message.starts_with("unknown sort \"foo\". Valid: sharpe, sortino, return_pct"), "{}", e.message);
        assert_eq!(parse_metric(" Sharpe ", "metric").ok().as_deref(), Some("sharpe"));
        assert_eq!(parse_metric("max_drawdown_pct", "metric").ok().as_deref(), Some("max_drawdown_pct"));
    }

    #[test]
    fn the_analysis_flag_reads_one_zero_and_yes_no() {
        for (raw, want) in [("1", true), ("yes", true), ("true", true), ("0", false), ("no", false), ("false", false)] {
            assert_eq!(query(&format!("analysis={raw}")).unwrap().analysis, Some(want), "{raw}");
        }
        assert_eq!(query("sort=sharpe").unwrap().analysis, None);
        assert!(query("analysis=maybe").is_err());
    }
}
