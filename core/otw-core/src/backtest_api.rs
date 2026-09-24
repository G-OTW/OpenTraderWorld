//! HTTP API for the Backtest module.
//!
//! - `POST /api/backtest/run`        run a strategy over one *or more* datasets (portfolio)
//! - `POST /api/backtest/align`      multi-asset alignment preview (no simulation)
//! - `GET  /api/backtest/runs`       run history (`?filter=saved` for the pinned ones)
//! - `POST /api/backtest/runs`       name a run (pins it; never pruned)
//! - `GET  /api/backtest/runs/{id}`  one recorded run (settings + stats)
//! - `DELETE /api/backtest/runs`     clear the auto history, keeping the named runs
//! - `DELETE /api/backtest/runs/{id}`
//!
//! Running is stateless — settings + dataset id(s) in, full result out — but every run is also
//! recorded in the history under a generated name, so a result outlives its HTTP response.
//! A stored row holds only the settings (to rerun) and the summary stats; trades and the
//! equity curve are recomputed on demand, which keeps the table small.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::backtest::{self, Bars, Settings};
use crate::quant;
use crate::{ApiError, AppState};
use otw_store::backtest as store;
use otw_store::histdata as hd;

/// Cap on the total simulated points `Σ (n_assets × bars)`. Keeps a portfolio run synchronous
/// and bounded (single-user; no job queue). Per-dataset load is also capped.
const MAX_TOTAL_BARS: usize = 400_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/run", post(run))
        .route("/api/backtest/sweep", post(sweep))
        .route("/api/backtest/align", post(align))
        .route("/api/backtest/runs", get(list_runs).post(save_run).delete(clear_runs))
        .route("/api/backtest/runs/{id}", get(get_run).delete(delete_run))
        .route("/api/backtest/runs/{id}/report.md", get(report_md))
        .route("/api/backtest/runs/{id}/report.pdf", get(report_pdf))
        .route("/api/backtest/runs/{id}/montecarlo", post(montecarlo))
        // Library: strategies + custom indicators (CRUD).
        .route("/api/backtest/strategies", get(list_strategies).post(create_strategy))
        .route(
            "/api/backtest/strategies/{id}",
            get(get_strategy).put(update_strategy).delete(delete_strategy),
        )
        .route("/api/backtest/indicators", get(list_indicators).post(create_indicator))
        .route(
            "/api/backtest/indicators/{id}",
            get(get_indicator).put(update_indicator).delete(delete_indicator),
        )
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RunBody {
    /// Legacy single dataset. Optional when `dataset_ids` is given.
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Portfolio dataset set (2–8). Falls back to `[dataset_id]` when absent.
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    /// Engine Settings JSON (same shape as a saved strategy's `settings`; GET one from
    /// /api/backtest/strategies/{id} for a working example).
    settings: Settings,
    /// Optional cap on bars loaded per dataset (defaults to a generous window). Applied to the
    /// MOST RECENT bars of the selected range — with a `from`/`to` window narrower than the
    /// cap it does nothing, but a cap smaller than the window silently keeps the tail.
    #[serde(default)]
    limit: Option<i64>,
    /// Start of the simulated window, inclusive. `YYYY-MM-DD` or RFC3339; a plain date is UTC
    /// midnight. Omit for "from the beginning of the dataset".
    #[serde(default)]
    from: Option<String>,
    /// End of the simulated window, inclusive. `YYYY-MM-DD` or RFC3339; a plain date covers
    /// that whole day (23:59:59Z), so `to: "2024-06-30"` keeps 30 June's bars. Omit for "to
    /// the end of the dataset".
    #[serde(default)]
    to: Option<String>,
    /// Set false to skip the history entry. Used by presentation reruns (the report route
    /// replays a stored run to rebuild its chart) so viewing a run never clones it.
    #[serde(default = "yes")]
    record: bool,
    /// Response shape. "full" (default) returns trades + equity + benchmark for the chart;
    /// "summary" returns stats and aggregates only — the per-trade list and the equity curve
    /// are omitted, which keeps an MCP agent's response inside its size budget. Either way
    /// the run lands in the history, so the full result stays reachable by run_id.
    #[serde(default)]
    view: Option<String>,
}

impl RunBody {
    /// Resolve the effective dataset id list from either field. Errors if none given.
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

    /// Resolve the date window, rejecting an inverted one. Both ends optional.
    fn window(&self) -> Result<Window, ApiError> {
        resolve_window(self.from.as_deref(), self.to.as_deref())
    }
}

/// A resolved `from`/`to` pair; `None` at either end means "unbounded that way".
type Window = (Option<OffsetDateTime>, Option<OffsetDateTime>);

/// A grid ladders one instrument: `run_portfolio` dispatches a grid run to `run_grid` with the
/// first dataset and the others are never simulated. Selecting a basket used to return that one
/// asset's result under a portfolio-looking breakdown, so the selection is rejected instead.
/// Shared by /run, /sweep and the optimizer.
pub(crate) fn check_single_asset(kind: &str, ids: &[Uuid]) -> Result<(), ApiError> {
    if kind == "grid" && ids.len() > 1 {
        return Err(ApiError::bad_request(
            "a grid strategy ladders one instrument: select a single dataset",
        ));
    }
    Ok(())
}

/// Strategy kind of a settings object still in raw JSON (sweep and optimizer variants).
pub(crate) fn settings_kind(settings: &Value) -> &str {
    settings.get("kind").and_then(Value::as_str).unwrap_or("signals")
}

/// Parse and validate a window. Shared by /run, /align and the optimizer so a preview or a sweep
/// can never accept a window the run would reject.
pub(crate) fn resolve_window(from: Option<&str>, to: Option<&str>) -> Result<Window, ApiError> {
    let from = from.map(|s| parse_bound(s, false)).transpose()?;
    let to = to.map(|s| parse_bound(s, true)).transpose()?;
    if let (Some(f), Some(t)) = (from, to) {
        if f >= t {
            return Err(ApiError::bad_request("`from` must be earlier than `to`"));
        }
    }
    Ok((from, to))
}

/// Parse a window bound. Accepts RFC3339 (`2024-06-30T12:00:00Z`) and a plain date
/// (`2024-06-30`). A plain date is UTC midnight, except as an END bound where it covers the
/// whole day — otherwise `to: "2024-06-30"` would drop every intraday bar of the 30th, which
/// is not what anyone means by "through June".
pub(crate) fn parse_bound(s: &str, end_of_day: bool) -> Result<OffsetDateTime, ApiError> {
    let s = s.trim();
    if let Ok(dt) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        return Ok(dt);
    }
    let date = time::Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|_| {
            ApiError::bad_request(&format!(
                "invalid date \"{s}\" — use YYYY-MM-DD or an RFC3339 timestamp"
            ))
        })?;
    let t = if end_of_day {
        time::Time::from_hms(23, 59, 59).expect("static time")
    } else {
        time::Time::MIDNIGHT
    };
    Ok(date.with_time(t).assume_utc())
}

fn yes() -> bool {
    true
}

/// Generated name for an auto history entry: tickers, timeframe and the run's wall-clock time
/// (mirrors what the UI's live-report save produces, e.g. "BTCUSDT 1h — 2026-07-19 14:32").
fn auto_run_name(assets: &[AssetArrays]) -> String {
    let tickers = if assets.len() > 1 {
        assets.iter().map(|a| a.ticker.as_str()).collect::<Vec<_>>().join("-")
    } else {
        assets[0].ticker.clone()
    };
    let now = time::OffsetDateTime::now_utc();
    let stamp = now
        .format(&time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]"
        ))
        .unwrap_or_default();
    format!("{tickers} {} — {stamp}", assets[0].timeframe)
}

/// One asset's OHLCV split into the parallel arrays the engine consumes. The engine owns the
/// shape (the optimizer holds the same buffers for the life of a sweep).
use crate::backtest::OwnedBars as AssetArrays;

/// Load each dataset, enforce identical timeframe + the bar budget, and split into arrays.
pub(crate) async fn load_assets(
    state: &AppState,
    ids: &[Uuid],
    limit: i64,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
) -> Result<Vec<AssetArrays>, ApiError> {
    let mut assets = Vec::with_capacity(ids.len());
    let mut timeframe: Option<String> = None;
    let mut total = 0usize;
    for &id in ids {
        let ds = hd::get_dataset(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("dataset not found"))?;
        match &timeframe {
            None => timeframe = Some(ds.timeframe.clone()),
            Some(tf) if *tf != ds.timeframe => {
                return Err(ApiError::bad_request(&format!(
                    "all datasets must share a timeframe ({} vs {})",
                    tf, ds.timeframe
                )));
            }
            _ => {}
        }
        let bars = hd::read_bars(&state.pool, id, from, to, limit).await?;
        if bars.len() < 2 {
            return Err(ApiError::bad_request(&format!(
                "{} has too few bars to backtest",
                ds.ticker
            )));
        }
        total += bars.len();
        if total > MAX_TOTAL_BARS {
            return Err(ApiError::bad_request(&format!(
                "too many bars: {} exceeds the {} point budget — narrow the range or drop a dataset",
                total, MAX_TOTAL_BARS
            )));
        }
        let ts = bars
            .iter()
            .map(|b| b.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default())
            .collect();
        assets.push(AssetArrays {
            ticker: ds.ticker,
            timeframe: ds.timeframe,
            ts,
            open: bars.iter().map(|b| b.open).collect(),
            high: bars.iter().map(|b| b.high).collect(),
            low: bars.iter().map(|b| b.low).collect(),
            close: bars.iter().map(|b| b.close).collect(),
            volume: bars.iter().map(|b| b.volume).collect(),
        });
    }
    Ok(assets)
}

// Demo sandbox: simulations are the CPU-heavy surface. The global budget bounds total load
// on the (2-core) demo host; the per-IP slice keeps one visitor from spending it all and
// pinning the box for everyone else. Applied to `run` and `montecarlo`.
static DEMO_SIM_QUOTA: crate::demo::WindowQuota =
    crate::demo::WindowQuota::new(30, 10, std::time::Duration::from_secs(600));

fn demo_sim_allowed(ip: Option<axum::Extension<crate::demo::ClientIp>>) -> Result<(), ApiError> {
    if crate::demo::enabled() {
        // Requests that bypassed the gate (in-process MCP dispatch) share one bucket.
        let ip = ip.map(|e| e.0 .0).unwrap_or_else(|| "internal".to_string());
        if !DEMO_SIM_QUOTA.allow(&ip) {
            return Err(ApiError::too_many(
                "the shared demo backtest budget is used up for now — try again in a few minutes",
            ));
        }
    }
    Ok(())
}

/// Load the dataset(s) and simulate the portfolio. Returns trades, equity, stats, per-asset
/// breakdown, warm-up info and (multi-asset) the alignment report.
async fn run(
    State(state): State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(raw): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    demo_sim_allowed(ip)?;
    // Taken as raw JSON first: `Settings` is deserialize-only, so the history snapshot keeps the
    // settings exactly as posted (a rerun replays them verbatim, with no round-trip drift).
    let settings_json = raw.get("settings").cloned().unwrap_or_else(|| json!({}));
    let body: RunBody = serde_json::from_value(raw)
        .map_err(|e| ApiError::bad_request(&format!("invalid run body: {e}")))?;
    let ids = body.ids()?;
    check_single_asset(&body.settings.kind, &ids)?;
    if let Some(err) = body.settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = body.window()?;
    let assets = load_assets(&state, &ids, limit, from, to).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    let result = backtest::run_portfolio(&body.settings, &bar_refs);

    let total_bars: usize = assets.iter().map(|a| a.ts.len()).sum();

    // Every run — UI or MCP agent — lands in the history under a generated name, so a result is
    // never lost once the response is gone. Naming it (POST /runs) is what pins it. A history
    // write must never fail the run itself: on error we log and return the result anyway.
    let stats_json = serde_json::to_value(&result.stats).unwrap_or_else(|_| json!({}));
    let run_id = if body.record {
        store::save_run(
            &state.pool,
            &store::NewRun {
                name: &auto_run_name(&assets),
                dataset_id: ids[0],
                dataset_ids: &ids,
                ticker: &assets[0].ticker,
                timeframe: &assets[0].timeframe,
                settings: &settings_json,
                stats: &stats_json,
                engine_version: backtest::ENGINE_VERSION as i32,
                strategy_id: None,
                pinned: false,
                bars_from: from,
                bars_to: to,
            },
        )
        .await
        .map_err(|e| tracing::warn!("backtest history write failed: {e}"))
        .ok()
    } else {
        None
    };

    // Legal settings that measure something other than the stated idea travel with the
    // result: the caller (agent or user) has to see them next to the numbers, not instead.
    let mut warnings = body.settings.warnings();
    // Weight-table mistakes only the loaded datasets can reveal: a ticker the basket does not
    // contain (which used to fall back to equal weights in silence), or an asset the table
    // forgot (a leg the plan then never buys).
    if let Some(cfg) = body.settings.dca.as_ref().filter(|_| body.settings.kind == "dca") {
        let tickers: Vec<&str> = assets.iter().map(|a| a.ticker.as_str()).collect();
        warnings.extend(backtest::dca::weight_warnings(cfg, &tickers));
    }

    // "summary" drops the two unbounded arrays (trades, equity) and the benchmark series.
    if body.view.as_deref() == Some("summary") {
        return Ok(Json(json!({
            "run_id": run_id,
            "ticker": assets[0].ticker,
            "timeframe": assets[0].timeframe,
            "bars": total_bars,
            "warnings": warnings,
            "stats": result.stats,
            "per_asset": result.per_asset,
            "warmup_bars": result.warmup_bars,
            "trading_start_ts": result.trading_start_ts,
            "alignment": result.alignment,
            "skipped_min_size": result.skipped_min_size,
            "skipped_margin": result.skipped_margin,
            "halted_bars": result.halted_bars,
            "filtered_bars": result.filtered_bars,
            "oos": result.oos,
            "total_funding": result.total_funding,
            "grid": result.grid,
            "dca": dca_headline(result.dca.as_ref()),
            // Counts stand in for the omitted arrays so the caller knows what it can fetch.
            "trades_omitted": result.trades.len(),
            "equity_points_omitted": result.equity.len(),
        })));
    }

    Ok(Json(json!({
        "run_id": run_id,
        // Legacy single-asset fields (kept for the current UI + saved-run history).
        "ticker": assets[0].ticker,
        "timeframe": assets[0].timeframe,
        "bars": total_bars,
        "warnings": warnings,
        "trades": result.trades,
        "equity": result.equity,
        "stats": result.stats,
        // Phase-1 additions.
        "per_asset": result.per_asset,
        "warmup_bars": result.warmup_bars,
        "trading_start_ts": result.trading_start_ts,
        "alignment": result.alignment,
        // Phase-2 additions (risk layer).
        "skipped_min_size": result.skipped_min_size,
        "skipped_margin": result.skipped_margin,
        "halted_bars": result.halted_bars,
        "filtered_bars": result.filtered_bars,
        "oos": result.oos,
        // Phase-4 additions (presentation).
        "benchmark": result.benchmark,
        "total_funding": result.total_funding,
        "grid": result.grid,
        "dca": result.dca,
    })))
}

// ── Sweep (server-side parameter grid) ───────────────────────────────────────────────────
//
// A grid search driven by the model is N sequential /run calls: it burns the tool-round cap,
// and only the caller ever knows how many variants were tried — which is exactly the number
// that decides whether the best result means anything. Running the grid here makes the trial
// count a property of the response instead of a claim in a reply.
//
// So this endpoint always returns EVERY trial, never just the winner, and it computes the
// deflated Sharpe against the trial count it actually ran.

/// Hard cap on trials per sweep. Each trial is a full simulation over the loaded bars.
const MAX_SWEEP_TRIALS: usize = 64;
/// Cap on axes — a 5-dimensional grid is a fishing expedition, not an experiment.
const MAX_SWEEP_AXES: usize = 4;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SweepBody {
    #[serde(default)]
    dataset_id: Option<Uuid>,
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    /// Base engine Settings; every trial is this with the grid values patched in.
    /// Patched as raw JSON (the grid writes into it before it is deserialized), but
    /// advertised with the real engine schema.
    #[schemars(with = "Settings")]
    settings: Value,
    /// The grid: dot-path into `settings` → the values to try. E.g.
    /// `{"long.stop_loss_pct": [0.01, 0.02, 0.03], "sizing.percent": [10, 25]}` = 6 trials.
    /// A numeric segment indexes an array, which is how indicator periods are reached:
    /// `long.entry.conditions.0.left.period`. Missing object keys are created; array indices
    /// must already exist.
    #[serde(default)]
    grid: std::collections::BTreeMap<String, Vec<Value>>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    /// Record each trial in the run history. Off by default: a 32-trial sweep would bury the
    /// history. Turn it on when you want a `run_id` per trial to reopen later.
    #[serde(default)]
    record: bool,
}

/// Trials a sweep body would run, for the agent's simulation budget (R11). Reads the grid
/// without running anything; `None` when the body is not a sweep body.
pub fn sweep_trial_count(body: Option<&Value>) -> Option<usize> {
    let grid = body?.get("grid")?.as_object()?;
    let n = grid
        .values()
        .filter_map(|v| v.as_array())
        .map(|a| a.len().max(1))
        .product::<usize>();
    Some(n.clamp(1, MAX_SWEEP_TRIALS))
}

/// Writing a swept value into the settings JSON is the optimizer's `set_path` — same grammar,
/// same array indexing, one implementation.
use crate::backtest::optimize::set_path;

/// Cartesian product of the grid axes, in a stable order (BTreeMap keys sort).
fn expand_grid(grid: &std::collections::BTreeMap<String, Vec<Value>>) -> Vec<Vec<(String, Value)>> {
    let mut out: Vec<Vec<(String, Value)>> = vec![Vec::new()];
    for (path, values) in grid {
        let mut next = Vec::with_capacity(out.len() * values.len().max(1));
        for combo in &out {
            for v in values {
                let mut c = combo.clone();
                c.push((path.clone(), v.clone()));
                next.push(c);
            }
        }
        out = next;
    }
    out
}

/// Run a parameter grid server-side and return the whole trial table.
async fn sweep(
    State(state): State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(body): Json<SweepBody>,
) -> Result<Json<Value>, ApiError> {
    demo_sim_allowed(ip)?;
    if body.grid.is_empty() {
        return Err(ApiError::bad_request(
            "a sweep needs a `grid`: {\"<settings.path>\": [values…]}. With no grid this is just \
             POST /api/backtest/run",
        ));
    }
    if body.grid.len() > MAX_SWEEP_AXES {
        return Err(ApiError::bad_request(&format!(
            "at most {MAX_SWEEP_AXES} grid axes — more than that is a fishing expedition, and \
             the multiple-testing haircut eats any result it finds"
        )));
    }
    let combos = expand_grid(&body.grid);
    if combos.len() > MAX_SWEEP_TRIALS {
        return Err(ApiError::bad_request(&format!(
            "that grid is {} trials; the cap is {MAX_SWEEP_TRIALS}. Narrow it — and remember \
             the reported result must be haircut for however many you do run",
            combos.len()
        )));
    }
    let ids = if !body.dataset_ids.is_empty() {
        body.dataset_ids.clone()
    } else if let Some(id) = body.dataset_id {
        vec![id]
    } else {
        return Err(ApiError::bad_request("no dataset selected"));
    };
    check_single_asset(settings_kind(&body.settings), &ids)?;

    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    // Loaded once and reused by every trial — the bars are the expensive part, and re-reading
    // them per trial would also let the window drift between trials.
    let assets = load_assets(&state, &ids, limit, from, to).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();

    let mut trials = Vec::with_capacity(combos.len());
    let mut sharpes: Vec<f64> = Vec::new();
    for combo in &combos {
        let mut settings_json = body.settings.clone();
        let mut params = serde_json::Map::new();
        for (path, val) in combo {
            set_path(&mut settings_json, path, val.clone()).map_err(|e| ApiError::bad_request(&e))?;
            params.insert(path.clone(), val.clone());
        }
        let settings: Settings = serde_json::from_value(settings_json.clone())
            .map_err(|e| ApiError::bad_request(&format!("trial settings are invalid: {e}")))?;
        if let Some(err) = settings.validate() {
            return Err(ApiError::bad_request(&format!("trial settings are invalid: {err}")));
        }
        let result = backtest::run_portfolio(&settings, &bar_refs);
        let run_id = if body.record {
            let stats_json = serde_json::to_value(&result.stats).unwrap_or_else(|_| json!({}));
            store::save_run(
                &state.pool,
                &store::NewRun {
                    name: &auto_run_name(&assets),
                    dataset_id: ids[0],
                    dataset_ids: &ids,
                    ticker: &assets[0].ticker,
                    timeframe: &assets[0].timeframe,
                    settings: &settings_json,
                    stats: &stats_json,
                    engine_version: backtest::ENGINE_VERSION as i32,
                    strategy_id: None,
                    pinned: false,
                    bars_from: from,
                    bars_to: to,
                },
            )
            .await
            .map_err(|e| tracing::warn!("sweep history write failed: {e}"))
            .ok()
        } else {
            None
        };
        if let Some(s) = result.stats.sharpe {
            if s.is_finite() {
                sharpes.push(s);
            }
        }
        // `return_pct` is net PnL over the money in, and a DCA axis can move the money in
        // itself: two trials are then percentages of different denominators. The figures that
        // make them comparable ride along, so the table can show what each trial paid in.
        let dca = result.dca.as_ref().map(|d| {
            json!({
                "contributed": d.contributed,
                "final_value": d.final_value,
                "twr_pct": d.twr_pct,
                "irr_pct": d.irr_pct,
            })
        });
        trials.push(json!({
            "params": params,
            "run_id": run_id,
            "trades": result.stats.trades,
            "net_pnl": result.stats.net_pnl,
            "return_pct": result.stats.return_pct,
            "sharpe": result.stats.sharpe,
            "sortino": result.stats.sortino,
            "max_drawdown_pct": result.stats.max_drawdown_pct,
            "profit_factor": result.stats.profit_factor,
            "win_rate": result.stats.win_rate,
            "oos": result.oos,
            "dca": dca,
        }));
    }

    let total_bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    let deflated =
        quant::deflated_sharpe(&sharpes, total_bars, quant::periods_per_year(&assets[0].timeframe));
    // `return_pct` is net PnL over the money in, and a DCA axis can move the money in itself:
    // ranking on it then rewards the trial that paid in the least, which is not a finding. It
    // is read off the trials rather than off the axis names, because a rule's threshold moves
    // the total just as much as the contribution does.
    let mut notes: Vec<String> = Vec::new();
    let paid_in: Vec<f64> = trials
        .iter()
        .filter_map(|t| t.get("dca")?.get("contributed")?.as_f64())
        .collect();
    let (lo, hi) = paid_in.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    if paid_in.len() == trials.len() && paid_in.len() > 1 && hi > lo * 1.001 {
        notes.push(format!(
            "the trials paid in between {lo:.0} and {hi:.0}, so `return_pct` is a percentage of \
             a different amount in each of them. Compare `twr_pct` (deposit-proof), or read \
             `dca.contributed` next to each row."
        ));
    }
    if trials.len() > 1 && sharpes.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-12) {
        notes.push(
            "every trial produced the same result: the swept paths are config this strategy              kind does not read. Check them against /api/backtest/optimize/params."
                .into(),
        );
    }
    Ok(Json(json!({
        "ticker": assets[0].ticker,
        "timeframe": assets[0].timeframe,
        "bars": total_bars,
        "trial_count": trials.len(),
        "grid": body.grid,
        "trials": trials,
        "notes": notes,
        "sharpe_distribution": quant::spread(&sharpes),
        "deflated_sharpe": deflated,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AlignBody {
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Settings drive the warm-up figure; optional (defaults to a no-indicator strategy).
    #[serde(default)]
    settings: Option<Settings>,
    #[serde(default)]
    limit: Option<i64>,
    /// Same window as the run this previews — otherwise the preview describes a different
    /// span than the simulation it is supposed to warn about. Same formats as on /run.
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

/// Cheap alignment preview — loads the bars but does NOT simulate. Powers the "alert the user
/// before running" banner (overlap window, warm-up, per-asset inactive-bar counts).
async fn align(
    State(state): State<AppState>,
    Json(body): Json<AlignBody>,
) -> Result<Json<Value>, ApiError> {
    let ids = if !body.dataset_ids.is_empty() {
        body.dataset_ids.clone()
    } else if let Some(id) = body.dataset_id {
        vec![id]
    } else {
        return Err(ApiError::bad_request("no dataset selected"));
    };
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref())?;
    let assets = load_assets(&state, &ids, limit, from, to).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    // A default no-signal strategy gives warmup 0 when the caller omits settings.
    let settings = body.settings.unwrap_or_else(default_align_settings);
    let report = backtest::align(&settings, &bar_refs);
    Ok(Json(json!({ "alignment": report })))
}

/// Minimal settings whose only purpose is a valid `warmup_bars` computation for /align when the
/// client sends no strategy yet. No sides ⇒ warm-up 0.
fn default_align_settings() -> Settings {
    serde_json::from_value(json!({
        "mode": "long",
        "sizing": { "mode": "fixed_qty", "qty": 1 }
    }))
    .expect("static default settings")
}

#[derive(Deserialize)]
struct RunsQuery {
    /// "saved" narrows the list to pinned runs; default returns the whole history.
    #[serde(default)]
    filter: Option<String>,
}

async fn list_runs(
    State(state): State<AppState>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Value>, ApiError> {
    let runs = store::list_runs(&state.pool, q.filter.as_deref() == Some("saved")).await?;
    Ok(Json(json!({ "runs": runs })))
}

/// Clear the auto history. Saved (pinned) runs are kept — those are deleted per-id.
async fn clear_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let deleted = store::delete_unpinned_runs(&state.pool).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveBody {
    name: String,
    /// Pin an existing history entry — the normal path, since every run is now recorded.
    /// Omit it to insert from the supplied settings/stats (older clients).
    #[serde(default)]
    run_id: Option<Uuid>,
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Full portfolio dataset set; defaults to `[dataset_id]` for single-asset saves.
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    /// Optional provenance link to the strategy the run came from.
    #[serde(default)]
    strategy_id: Option<Uuid>,
    #[serde(default)]
    settings: Value,
    #[serde(default)]
    stats: Value,
    /// Window the run covered, when it was a windowed run. Only read on the legacy insert
    /// path — pinning by `run_id` keeps whatever the recorded row already holds.
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

async fn save_run(
    State(state): State<AppState>,
    Json(body): Json<SaveBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    // Normal path: the run is already in the history, so naming it is an update.
    if let Some(run_id) = body.run_id {
        if !store::pin_run(&state.pool, run_id, name).await? {
            return Err(ApiError::not_found("run not found"));
        }
        return Ok(Json(json!({ "id": run_id })));
    }
    let dataset_id = body
        .dataset_id
        .or_else(|| body.dataset_ids.first().copied())
        .ok_or_else(|| ApiError::bad_request("run_id or dataset_id is required"))?;
    let ds = hd::get_dataset(&state.pool, dataset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let dataset_ids = if body.dataset_ids.is_empty() {
        vec![dataset_id]
    } else {
        body.dataset_ids.clone()
    };
    // The stats snapshot carries the engine version it was produced under; fall back to the
    // current engine for older client payloads that omit it.
    let engine_version = body
        .stats
        .get("engine_version")
        .and_then(Value::as_u64)
        .map(|v| v as i32)
        .unwrap_or(backtest::ENGINE_VERSION as i32);
    let id = store::save_run(
        &state.pool,
        &store::NewRun {
            name,
            dataset_id,
            dataset_ids: &dataset_ids,
            ticker: &ds.ticker,
            timeframe: &ds.timeframe,
            settings: &body.settings,
            stats: &body.stats,
            engine_version,
            strategy_id: body.strategy_id,
            pinned: true,
            bars_from: body.from.as_deref().map(|s| parse_bound(s, false)).transpose()?,
            bars_to: body.to.as_deref().map(|s| parse_bound(s, true)).transpose()?,
        },
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

/// One recorded run: the settings it was simulated with and the stats it produced. The
/// history list answers "what did I run", this answers "what exactly was in it" without
/// replaying anything — trades and the equity curve stay out, as they do in the list.
async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    Ok(Json(json!({ "run": run })))
}

async fn delete_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_run(&state.pool, id).await? {
        return Err(ApiError::not_found("run not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Everything a report shows beyond the stored snapshot (the equity curve, the per-asset
/// split, the OOS blocks, the execution counters) comes from replaying the run over its
/// own datasets and window. The replay is deterministic (same settings, same bars), records
/// nothing, and returns `Null` when it can't happen (dataset deleted, settings from another
/// engine shape): the report then falls back to the snapshot alone.
async fn replay_extra(state: &AppState, run: &store::SavedRun) -> Value {
    let ids: Vec<Uuid> = match run.dataset_ids.as_deref() {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => match run.dataset_id {
            Some(id) => vec![id],
            None => return Value::Null,
        },
    };
    let settings: Settings = match serde_json::from_value(run.settings.clone()) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("report replay skipped, settings unreadable: {e}");
            return Value::Null;
        }
    };
    // Same bar budget the /run endpoint defaults to, so a report replays the run the UI made.
    let assets = match load_assets(state, &ids, 50_000, run.bars_from, run.bars_to).await {
        Ok(a) => a,
        Err(e) => {
            tracing::debug!("report replay skipped, datasets unavailable: {}", e.message);
            return Value::Null;
        }
    };
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let refs: Vec<&Bars> = bars.iter().collect();
    let r = backtest::run_portfolio(&settings, &refs);
    json!({
        "stats": r.stats,
        "bars": assets.iter().map(|a| a.ts.len()).sum::<usize>(),
        "first_ts": assets.iter().filter_map(|a| a.ts.first()).min(),
        "last_ts": assets.iter().filter_map(|a| a.ts.last()).max(),
        "equity": r.equity,
        // Lean projection of the closed trades: what the report's statistics tables are
        // derived from. The trade list itself is never printed.
        "trades": r.trades.iter().map(|t| json!({
            "ticker": t.ticker,
            "direction": t.direction,
            "pnl": t.pnl,
            "return_pct": t.return_pct,
            "mae": t.mae,
            "mfe": t.mfe,
            "bars_held": t.bars_held,
        })).collect::<Vec<_>>(),
        "per_asset": r.per_asset,
        "oos": r.oos,
        "grid": r.grid,
        "dca": dca_headline(r.dca.as_ref()),
        "warmup_bars": r.warmup_bars,
        "trading_start_ts": r.trading_start_ts,
        "skipped_min_size": r.skipped_min_size,
        "skipped_margin": r.skipped_margin,
        "halted_bars": r.halted_bars,
        "filtered_bars": r.filtered_bars,
        "total_funding": r.total_funding,
    })
}

/// The DCA block without its three curves and its fill list: the headline a summary view (or a
/// report replay) reads, with the unbounded arrays left behind.
fn dca_headline(d: Option<&backtest::dca::DcaStats>) -> Value {
    let Some(d) = d else { return Value::Null };
    let mut v = serde_json::to_value(d).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        for k in ["contributed_curve", "cost_curve", "events"] {
            o.remove(k);
        }
        // `events_total` stays: it is the count, not the list, and a summary reader needs to
        // know how many fills the plan made.
    }
    v
}

/// A report file is named after the strategy it came from, plus the moment it was exported,
/// so two exports of the same strategy never collide: `my_strategy_20260818-094500`. A run
/// with no strategy behind it falls back to `otw_strategy`.
fn report_filename(strategy: Option<&str>, ext: &str) -> String {
    let stamp = OffsetDateTime::now_utc()
        .format(&time::macros::format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))
        .unwrap_or_default();
    let base = strategy.map(slug).filter(|s| !s.is_empty()).unwrap_or_else(|| "otw_strategy".into());
    format!("{base}_{stamp}.{ext}")
}

/// Filename-safe form of a user-chosen name: ASCII word characters, everything else folded to
/// a single underscore.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').chars().take(60).collect()
}

/// Name of the strategy a run came from, when it still exists.
async fn run_strategy_name(state: &AppState, run: &store::SavedRun) -> Option<String> {
    let id = run.strategy_id?;
    store::get_strategy(&state.pool, id).await.ok().flatten().map(|s| s.name)
}

/// Markdown report for a saved run: the results screen as a document (headline stats, curve,
/// strategy, All/Long/Short performance, OOS, per-asset) and never the trade list.
async fn report_md(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let extra = replay_extra(&state, &run).await;
    let doc = crate::backtest::report::RunDoc::new(
        &run.name,
        &run.ticker,
        &run.timeframe,
        &run.settings,
        &run.stats,
    )
    .with_extra(&extra);
    let md = crate::backtest::report::run_report_md(&doc);
    let filename = report_filename(run_strategy_name(&state, &run).await.as_deref(), "md");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        md,
    )
        .into_response())
}

/// Same report as `report.md`, rendered to PDF by the shared report engine.
async fn report_pdf(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let extra = replay_extra(&state, &run).await;
    let doc = crate::backtest::report::RunDoc::new(
        &run.name,
        &run.ticker,
        &run.timeframe,
        &run.settings,
        &run.stats,
    )
    .with_extra(&extra);
    let pdf = crate::report::pdf::render(&crate::backtest::report::run_report(&doc));
    let safe: String = run
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"backtest_{safe}.pdf\""),
            ),
        ],
        pdf,
    )
        .into_response())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct MonteCarloBody {
    /// Simulated paths. Clamped in the engine.
    #[serde(default = "default_mc_iterations")]
    iterations: usize,
    /// Trades per path. `None`/0 → use the run's own trade count.
    #[serde(default)]
    horizon: Option<usize>,
    /// Resampling block length; 1 = IID bootstrap, >1 = streak-preserving block bootstrap.
    #[serde(default = "default_mc_block")]
    block: usize,
    /// Ruin threshold as a fraction of starting capital (0.5 = a 50% drawdown from start).
    #[serde(default = "default_mc_ruin")]
    ruin_pct: f64,
    /// Response shape. "full" (default) returns the equity fan, both histograms and the
    /// realized curve — one point per horizon step or bucket, which is what the chart needs
    /// and what overflows an agent's response budget. "summary" keeps the figures that carry
    /// the answer: percentiles, risk of ruin, probability of loss, median return.
    #[serde(default)]
    view: Option<String>,
}

fn default_mc_iterations() -> usize {
    5_000
}
fn default_mc_block() -> usize {
    1
}
fn default_mc_ruin() -> f64 {
    0.5
}

/// Monte-Carlo resampling of a saved run's realized per-trade P&L (spec §2, option A: rerun the
/// stored settings to regenerate the exact trade sequence — nothing extra is persisted). Draws
/// many equity paths → drawdown / risk-of-ruin bands.
async fn montecarlo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(body): Json<MonteCarloBody>,
) -> Result<Json<Value>, ApiError> {
    demo_sim_allowed(ip)?;
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;

    // Rebuild the engine Settings from the stored snapshot; a shape mismatch (older engine) is a
    // clear client-facing error rather than a 500.
    let settings: Settings = serde_json::from_value(run.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("run settings are not replayable: {e}")))?;
    let starting_capital = settings.starting_capital;

    // Resolve the dataset set (portfolio runs carry `dataset_ids`; legacy rows use `dataset_id`).
    let ids: Vec<Uuid> = match run.dataset_ids {
        Some(v) if !v.is_empty() => v,
        _ => run.dataset_id.map(|d| vec![d]).unwrap_or_default(),
    };
    if ids.is_empty() {
        return Err(ApiError::bad_request("run has no dataset to replay"));
    }

    // Replay the run to regenerate its exact trade list — over the SAME window it was run
    // over, or the resampled P&L would not be this run's.
    let assets = load_assets(&state, &ids, 200_000, run.bars_from, run.bars_to).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    let result = backtest::run_portfolio(&settings, &bar_refs);

    let pnls: Vec<f64> = result.trades.iter().map(|t| t.pnl).collect();
    if pnls.len() < 2 {
        return Err(ApiError::bad_request(
            "this run has too few trades to resample (need at least 2)",
        ));
    }

    let horizon = body.horizon.filter(|h| *h > 0).unwrap_or(pnls.len());
    // Seed off the run id so repeated calls on the same run are reproducible.
    let seed = id.as_u128() as u64 ^ (id.as_u128() >> 64) as u64;
    let mc = quant::monte_carlo(
        &pnls,
        starting_capital,
        body.iterations,
        horizon,
        body.block,
        body.ruin_pct,
        seed,
    );

    // "summary" drops the four unbounded arrays. The scalars below are the whole answer to
    // "how else could this have gone"; the arrays only exist to draw it.
    if body.view.as_deref() == Some("summary") {
        return Ok(Json(json!({
            "name": run.name,
            "ticker": run.ticker,
            "timeframe": run.timeframe,
            "result": {
                "source_trades": mc.source_trades,
                "horizon": mc.horizon,
                "iterations": mc.iterations,
                "start_capital": mc.start_capital,
                "method": mc.method,
                "final_equity": mc.final_equity,
                "max_drawdown": mc.max_drawdown,
                "risk_of_ruin": mc.risk_of_ruin,
                "ruin_level": mc.ruin_level,
                "prob_loss": mc.prob_loss,
                "median_return": mc.median_return,
            },
        })));
    }

    Ok(Json(json!({
        "name": run.name,
        "ticker": run.ticker,
        "timeframe": run.timeframe,
        "result": mc,
    })))
}

// ── Strategies (named Settings) ──────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StrategyBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Free-form labels; the library's search box matches them alongside the name.
    #[serde(default)]
    tags: Vec<String>,
    /// Stored raw (the handler validates it by deserializing into `Settings` separately), but
    /// advertised with the real engine schema — an MCP agent handed an untyped `settings`
    /// invents field names, which is exactly how `mode: "percent_equity"` happens.
    #[schemars(with = "Settings")]
    settings: Value,
}

/// Map a unique-name violation to a friendly 400 instead of a 500.
fn name_taken(err: anyhow::Error, what: &str) -> ApiError {
    let s = err.to_string();
    if s.contains("duplicate key") || s.contains("unique") {
        ApiError::bad_request(&format!("a {what} with that name already exists"))
    } else {
        ApiError::internal(&s)
    }
}

async fn list_strategies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "strategies": store::list_strategies(&state.pool).await? })))
}

async fn get_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_strategy(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("strategy not found"))?;
    Ok(Json(json!({ "strategy": s })))
}

/// Validate a strategy body: non-empty name + the settings must deserialize + pass engine checks.
fn check_strategy(body: &StrategyBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("strategy name is required"));
    }
    let settings: Settings = serde_json::from_value(body.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid strategy settings: {e}")))?;
    if let Some(err) = settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    Ok(())
}

async fn create_strategy(
    State(state): State<AppState>,
    Json(body): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    check_strategy(&body)?;
    let id = store::create_strategy(
        &state.pool,
        body.name.trim(),
        body.description.trim(),
        &body.tags,
        &body.settings,
    )
    .await
    .map_err(|e| name_taken(e, "strategy"))?;
    Ok(Json(json!({ "id": id })))
}

async fn update_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    check_strategy(&body)?;
    let ok = store::update_strategy(
        &state.pool,
        id,
        body.name.trim(),
        body.description.trim(),
        &body.tags,
        &body.settings,
    )
    .await
    .map_err(|e| name_taken(e, "strategy"))?;
    if !ok {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_strategy(&state.pool, id).await? {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Custom indicators (node-graph DAG) ───────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct IndicatorBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Stored raw, validated separately — advertised with the real node-DAG schema.
    #[schemars(with = "backtest::CustomIndicatorDef")]
    definition: Value,
}

async fn list_indicators(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "indicators": store::list_indicators(&state.pool).await? })))
}

async fn get_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let i = store::get_indicator(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("indicator not found"))?;
    Ok(Json(json!({ "indicator": i })))
}

/// Validate an indicator body: non-empty name + the definition must deserialize into the node
/// graph and pass structural validation (bounded size, no forward/self references).
fn check_indicator(body: &IndicatorBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("indicator name is required"));
    }
    let def: backtest::CustomIndicatorDef = serde_json::from_value(body.definition.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid indicator definition: {e}")))?;
    if let Some(err) = def.validate() {
        return Err(ApiError::bad_request(&err));
    }
    Ok(())
}

async fn create_indicator(
    State(state): State<AppState>,
    Json(body): Json<IndicatorBody>,
) -> Result<Json<Value>, ApiError> {
    check_indicator(&body)?;
    let id = store::create_indicator(&state.pool, body.name.trim(), body.description.trim(), &body.definition)
        .await
        .map_err(|e| name_taken(e, "indicator"))?;
    Ok(Json(json!({ "id": id })))
}

async fn update_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<IndicatorBody>,
) -> Result<Json<Value>, ApiError> {
    check_indicator(&body)?;
    let ok = store::update_indicator(&state.pool, id, body.name.trim(), body.description.trim(), &body.definition)
        .await
        .map_err(|e| name_taken(e, "indicator"))?;
    if !ok {
        return Err(ApiError::not_found("indicator not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_indicator(&state.pool, id).await? {
        return Err(ApiError::not_found("indicator not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod sweep_tests {
    use super::*;

    fn grid(pairs: &[(&str, Vec<Value>)]) -> std::collections::BTreeMap<String, Vec<Value>> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn expands_the_cartesian_product() {
        let g = grid(&[
            ("a", vec![json!(1), json!(2)]),
            ("b", vec![json!("x"), json!("y"), json!("z")]),
        ]);
        let combos = expand_grid(&g);
        assert_eq!(combos.len(), 6);
        // Every combination is distinct and complete.
        let mut seen: Vec<String> = combos.iter().map(|c| format!("{c:?}")).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 6);
        assert!(combos.iter().all(|c| c.len() == 2));
    }

    /// The budget is charged before the sweep runs, so the count has to come from the body
    /// alone — the same arithmetic the endpoint will do.
    #[test]
    fn trial_count_reads_the_grid_without_running_it() {
        let body = json!({ "grid": { "a": [1, 2, 3], "b": [10, 20] } });
        assert_eq!(sweep_trial_count(Some(&body)), Some(6));
        // Not a sweep body at all.
        assert_eq!(sweep_trial_count(Some(&json!({ "settings": {} }))), None);
        assert_eq!(sweep_trial_count(None), None);
        // An oversized grid is charged at the cap; the endpoint rejects it separately.
        let big = json!({ "grid": { "a": (0..100).collect::<Vec<_>>() } });
        assert_eq!(sweep_trial_count(Some(&big)), Some(MAX_SWEEP_TRIALS));
    }

    #[test]
    fn sets_nested_paths_and_creates_missing_objects() {
        let mut v = json!({ "long": { "stop_loss_pct": 0.01 } });
        set_path(&mut v, "long.stop_loss_pct", json!(0.03)).unwrap();
        set_path(&mut v, "sizing.percent", json!(25)).unwrap();
        assert_eq!(v["long"]["stop_loss_pct"], json!(0.03));
        assert_eq!(v["sizing"]["percent"], json!(25));
    }

    /// A path through a scalar is a typo, not a request to replace the scalar with an object.
    #[test]
    fn refuses_a_path_through_a_scalar() {
        let mut v = json!({ "slippage": 0.0005 });
        assert!(set_path(&mut v, "slippage.value.kind", json!("pct")).is_err());
        assert!(set_path(&mut v, "", json!(1)).is_err());
    }

    /// The grid anyone actually wants: an indicator period, which lives inside a conditions
    /// array. Without array indexing a sweep can only reach costs and sizing.
    #[test]
    fn indexes_into_arrays() {
        let mut v = json!({
            "long": { "entry": { "conditions": [
                { "left": { "indicator": "sma", "period": 20 },
                  "right": { "indicator": "sma", "period": 50 } }
            ]}}
        });
        set_path(&mut v, "long.entry.conditions.0.left.period", json!(10)).unwrap();
        assert_eq!(v["long"]["entry"]["conditions"][0]["left"]["period"], json!(10));
        // The sibling is untouched.
        assert_eq!(v["long"]["entry"]["conditions"][0]["right"]["period"], json!(50));
        // Replacing a whole array element works too.
        set_path(&mut v, "long.entry.conditions.0", json!({"x": 1})).unwrap();
        assert_eq!(v["long"]["entry"]["conditions"][0], json!({"x": 1}));
    }

    /// A grid sweeps what exists; it does not invent conditions.
    #[test]
    fn refuses_an_index_past_the_end() {
        let mut v = json!({ "conditions": [{ "period": 20 }] });
        let e = set_path(&mut v, "conditions.3.period", json!(9)).unwrap_err();
        assert!(e.contains("past the end"), "{e}");
        let e = set_path(&mut v, "conditions.first.period", json!(9)).unwrap_err();
        assert!(e.contains("array index"), "{e}");
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;

    fn rfc(s: &str) -> OffsetDateTime {
        OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap()
    }
    /// ApiError carries no Debug, so unwrap the message by hand rather than deriving one
    /// just for tests.
    fn ok<T>(r: Result<T, ApiError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("expected Ok, got: {}", e.message),
        }
    }
    fn err_msg<T>(r: Result<T, ApiError>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.message,
        }
    }

    #[test]
    fn parses_rfc3339_verbatim() {
        assert_eq!(ok(parse_bound("2024-06-30T12:34:56Z", false)), rfc("2024-06-30T12:34:56Z"));
        // end_of_day must not touch an explicit timestamp.
        assert_eq!(ok(parse_bound("2024-06-30T12:34:56Z", true)), rfc("2024-06-30T12:34:56Z"));
    }

    /// A plain end date covers its whole day — the trap being that `to: "2024-06-30"` read as
    /// midnight would silently drop every intraday bar of the 30th.
    #[test]
    fn plain_date_end_bound_covers_the_day() {
        assert_eq!(ok(parse_bound("2024-06-30", false)), rfc("2024-06-30T00:00:00Z"));
        assert_eq!(ok(parse_bound("2024-06-30", true)), rfc("2024-06-30T23:59:59Z"));
    }

    #[test]
    fn report_files_are_named_after_the_strategy() {
        let named = report_filename(Some("My BTC Scalper v2"), "md");
        assert!(named.starts_with("my_btc_scalper_v2_"), "{named}");
        assert!(named.ends_with(".md"));
        // strategy + "_" + 8 digits + "-" + 6 digits + ".md"
        assert_eq!(named.len(), "my_btc_scalper_v2".len() + 1 + 15 + 3);
        assert!(report_filename(None, "md").starts_with("otw_strategy_"));
        assert!(report_filename(Some("   "), "md").starts_with("otw_strategy_"));
        // Anything outside ASCII word characters folds to one underscore.
        assert_eq!(slug("Ünïcode / slashes"), "n_code_slashes");
    }

    #[test]
    fn rejects_junk_with_a_usable_message() {
        assert!(err_msg(parse_bound("30/06/2024", false)).contains("YYYY-MM-DD"));
        assert!(parse_bound("", false).is_err());
        assert!(parse_bound("2024-13-01", false).is_err());
    }

    #[test]
    fn window_is_optional_at_both_ends() {
        assert_eq!(ok(resolve_window(None, None)), (None, None));
        assert_eq!(
            ok(resolve_window(Some("2024-01-01"), None)),
            (Some(rfc("2024-01-01T00:00:00Z")), None)
        );
        assert_eq!(
            ok(resolve_window(None, Some("2024-01-01"))),
            (None, Some(rfc("2024-01-01T23:59:59Z")))
        );
    }

    /// An inverted window loads zero bars and would otherwise fail deep in the loader as
    /// "too few bars to backtest" — catch it where the message can name the real cause.
    #[test]
    fn rejects_inverted_window() {
        assert!(err_msg(resolve_window(Some("2024-06-01"), Some("2024-01-01"))).contains("earlier"));
        assert!(resolve_window(Some("2024-06-01T00:00:00Z"), Some("2024-06-01T00:00:00Z")).is_err());
        // The same plain date at both ends is a valid one-day window (00:00 → 23:59:59).
        assert!(resolve_window(Some("2024-06-01"), Some("2024-06-01")).is_ok());
    }
}
