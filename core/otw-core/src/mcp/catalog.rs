//! MCP endpoint allowlist — the only REST routes reachable through the gateway tools.
//!
//! Security model: explicit allowlist, not passthrough. Anything absent here is
//! unreachable via MCP regardless of token permissions. Deliberately excluded:
//! account/session/settings/network admin, secret management (feed + provider API
//! keys, notification channels), data wipe, module install/detach, logs, binary
//! file upload/download, SSE streams, the FinanceDatabase bulk import, the
//! community-docs external submission relay, the in-app agent (it holds provider
//! keys and would reach this gateway back), inbound-webhook management (it mints
//! secrets), and the two bulk importers (journal, portfolios).
//!
//! The Automator is here for the authoring half only: read a workflow, create one, save a
//! graph, test it. Running one, restoring a revision, deleting one, and every schedule
//! endpoint stay out, because a workflow executes under its own `mcp_tokens` envelope and
//! a caller that could both write a graph and set it running would inherit whatever that
//! envelope grants, whatever its own token says. `automator_api` completes the split: an
//! automated graph save lands in the draft the engine never reads, so what runs is always
//! a graph a human saved.
//!
//! Binary responses stay out for a mechanical reason: the gateway stringifies a
//! response body with `from_utf8_lossy`, so a PDF arrives as mojibake. That is why
//! a run report is reachable as `report.md` and not as `report.pdf`.
//!
//! `module` is the permission key tokens are scoped by (aligned with the frontend
//! module ids). GET needs `"r"`; other methods need `"rw"`.
//!
//! Some responses carry text nobody here wrote: feed articles, incoming mail. Those
//! paths are listed in [`UNTRUSTED_PATHS`] and the gateway fences their body before the
//! model sees it.
//!
//! One key is not a module: `search` reads titles across every module at once, so
//! granting it is granting title-level read everywhere, whatever the other keys say.
//! It is separate precisely so that decision is taken on purpose.
//!
//! Request-body contract: every POST/PUT/PATCH entry carries a JSON Schema generated
//! (via schemars) from the exact struct its Axum handler deserializes, so what the
//! catalog advertises is what serde accepts — the schema cannot drift from the code.
//! The constructors enforce this at compile time: `post`/`put`/`patch` require a
//! schema, and a write endpoint that genuinely takes no body must say so with
//! `post_empty`. A GET never takes a body; a DELETE only does through
//! `delete_body`, for the handful of handlers that identify their target by payload
//! rather than by path.

use serde_json::Value;

/// Generator for an endpoint's request-body JSON Schema.
pub type BodySchema = fn() -> Value;

pub struct Endpoint {
    pub module: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub desc: &'static str,
    /// JSON Schema of the request body; `None` = the endpoint takes no body.
    pub body: Option<BodySchema>,
    /// A POST that computes rather than mutates: it answers a question and changes nothing
    /// the user would miss. These skip the write-confirmation step — asking someone to
    /// approve a Sharpe ratio trains them to click through the prompt that matters. Remote
    /// clients reach them through the `otw_compute` tool, the in-app agent reads this flag
    /// directly; both consult the same list.
    ///
    /// `/backtest/run` and `/backtest/sweep` count as compute even though they append to the
    /// run history: that history is the agent's own audit trail, prunable in one click, and
    /// confirming every simulation would make the quant persona unusable.
    pub compute: bool,
}

/// JSON Schema for a handler's request-body struct; `$schema`/`title` noise is stripped.
///
/// Named subschemas stay under `definitions` and are referenced by `$ref` rather than
/// inlined: the engine `Settings` embeds the same operand/signal types dozens of times and
/// appears on four endpoints of one module, so inlining turns a module listing into hundreds
/// of kilobytes. [`crate::mcp::render_module`] hoists these into one per-page block.
pub fn schema<T: schemars::JsonSchema>() -> Value {
    let settings = schemars::generate::SchemaSettings::draft07();
    let root = settings.into_generator().into_root_schema_for::<T>();
    let mut v = serde_json::to_value(root).expect("schema serializes");
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
    }
    v
}

const fn get(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "GET", path, desc, body: None, compute: false }
}

const fn delete(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "DELETE", path, desc, body: None, compute: false }
}

/// A DELETE whose target is named by the request body, not by the path.
const fn delete_body(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "DELETE", path, desc, body: Some(body), compute: false }
}

const fn post(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: Some(body), compute: false }
}

/// A POST that answers a question instead of storing something — see [`Endpoint::compute`].
const fn compute_post(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: Some(body), compute: true }
}

/// A POST that deliberately takes no request body (action is fully named by the path).
const fn post_empty(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: None, compute: false }
}

/// A no-body POST that computes — see [`Endpoint::compute`].
const fn compute_post_empty(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: None, compute: true }
}

const fn put(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "PUT", path, desc, body: Some(body), compute: false }
}

const fn patch(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "PATCH", path, desc, body: Some(body), compute: false }
}

/// Human labels for the permission UI, in display order.
pub const MODULES: &[(&str, &str)] = &[
    ("journal", "Trading Journal"),
    ("portfolios", "Portfolio Tracker"),
    ("watchlists", "Watchlists"),
    ("backtest", "Backtest"),
    ("quant", "Quant Tools"),
    ("histdata", "Historical Data"),
    ("connectors", "Data connectors"),
    ("findb", "Product Search"),
    ("mportfolios", "Managers' Portfolios"),
    ("wealth", "MyWealth"),
    ("subscriptions", "Subscriptions"),
    ("taxcalc", "Tax Calculator"),
    ("editor", "Editor"),
    ("todos", "ToDo List"),
    ("goals", "Goals"),
    ("remindme", "RemindMe"),
    ("calendar", "Calendar"),
    ("time", "Time Tracker"),
    ("routines", "Trader Routines"),
    ("mindset", "Mindset"),
    ("news", "News Feeds"),
    ("resources", "Resources"),
    ("prompt-store", "Prompt Store"),
    ("community-docs", "Community Docs"),
    ("mailbox", "Mailbox"),
    ("automator", "Automator"),
    ("search", "Global search"),
];

pub const CATALOG: &[Endpoint] = &[
    // ── journal ──────────────────────────────────────────────────────────────
    get("journal", "/api/journal/trades", "List trades. Query: status, category_id, strategy_id, symbol, from, to, limit, offset."),
    post("journal", "/api/journal/trades", "Create a trade (template fields + typed cols: symbol, side, qty, prices, fees…).", schema::<otw_store::journal::TradeInput>),
    get("journal", "/api/journal/trades/{id}", "Get one trade with legs/brackets and computed PnL."),
    patch("journal", "/api/journal/trades/{id}", "Update a trade (full payload, same shape as create).", schema::<otw_store::journal::TradeInput>),
    delete("journal", "/api/journal/trades/{id}", "Delete a trade."),
    get("journal", "/api/journal/trade-suggestions", "Autocomplete values seen in past trades (symbols…)."),
    get("journal", "/api/journal/breakdown", "Stats/PnL breakdown. Query: from, to, category_id, group (day|week|month|strategy|symbol…)."),
    get("journal", "/api/journal/calendar", "Daily realized-PnL buckets for the month-grid heatmap. Same filters as breakdown."),
    get("journal", "/api/journal/analytics", "R-multiples, streaks, Sharpe/Sortino, hold-time / hour / weekday / size distributions, per-strategy-symbol-tag tables and the cost of tagged mistakes. Same filters as breakdown, plus tz_offset."),
    get("journal", "/api/journal/compare", "One period against the previous one plus a 12-period strip. Query: period (day|week|month|quarter|year|custom), anchor, tz_offset, same filters as breakdown."),
    get("journal", "/api/journal/tags", "List discipline tags (kind: mistake | rule | setup) with their trade counts."),
    post("journal", "/api/journal/tags", "Create a discipline tag.", schema::<otw_store::journal_analytics::TagInput>),
    patch("journal", "/api/journal/tags/{id}", "Update a discipline tag.", schema::<otw_store::journal_analytics::TagPatch>),
    delete("journal", "/api/journal/tags/{id}", "Delete a discipline tag."),
    get("journal", "/api/journal/categories", "List trade categories (accounts/books)."),
    post("journal", "/api/journal/categories", "Create a category.", schema::<crate::journal_api::CategoryBody>),
    patch("journal", "/api/journal/categories/{id}", "Update a category.", schema::<otw_store::journal::CategoryPatch>),
    delete("journal", "/api/journal/categories/{id}", "Delete a category."),
    get("journal", "/api/journal/categories/{id}/capital", "List capital events (deposits/withdrawals) for a category."),
    post("journal", "/api/journal/categories/{id}/capital", "Add a capital event.", schema::<crate::journal_api::CapitalBody>),
    delete("journal", "/api/journal/capital/{id}", "Delete a capital event."),
    get("journal", "/api/journal/strategies", "List strategies."),
    post("journal", "/api/journal/strategies", "Create a strategy.", schema::<crate::journal_api::StrategyBody>),
    patch("journal", "/api/journal/strategies/{id}", "Update a strategy.", schema::<otw_store::journal::StrategyPatch>),
    delete("journal", "/api/journal/strategies/{id}", "Delete a strategy."),
    get("journal", "/api/journal/templates", "List trade-form templates."),
    post("journal", "/api/journal/templates", "Create a template.", schema::<crate::journal_api::TemplateBody>),
    patch("journal", "/api/journal/templates/{id}", "Update a template.", schema::<otw_store::journal::TemplatePatch>),
    delete("journal", "/api/journal/templates/{id}", "Delete a template."),
    get("journal", "/api/journal/fee-schedules", "List fee schedules."),
    post("journal", "/api/journal/fee-schedules", "Create a fee schedule.", schema::<otw_store::journal::FeeScheduleInput>),
    patch("journal", "/api/journal/fee-schedules/{id}", "Update a fee schedule.", schema::<otw_store::journal::FeeSchedulePatch>),
    delete("journal", "/api/journal/fee-schedules/{id}", "Delete a fee schedule."),
    get("journal", "/api/journal/settings", "Journal settings (display currency…)."),
    patch("journal", "/api/journal/settings", "Update journal settings.", schema::<crate::journal_api::SettingsBody>),
    get("journal", "/api/journal/export/trades.csv", "Every trade of the filtered set as CSV text (same filters as /trades). Large: prefer /breakdown or /report unless the raw rows are the point."),
    get("journal", "/api/journal/fx/pending", "FX conversions awaiting a manual rate."),
    get("journal", "/api/journal/fx/quotes", "Latest FX quotes (USD-based)."),
    get("journal", "/api/journal/fx/rates/{date}", "FX rates on a date (YYYY-MM-DD)."),
    post("journal", "/api/journal/fx/rates/{date}", "Resolve a pending date with manual rates.", schema::<crate::fx_api::ResolveBody>),
    // ── portfolios ───────────────────────────────────────────────────────────
    get("portfolios", "/api/portfolios", "List portfolios with valuation summary."),
    post("portfolios", "/api/portfolios", "Create a portfolio.", schema::<crate::portfolios_api::CreateBody>),
    get("portfolios", "/api/portfolios/search", "Search priceable assets. Query: q (REQUIRED), kind (REQUIRED, crypto|stock — \"stock\" also returns ETFs)."),
    get("portfolios", "/api/portfolios/{id}", "Portfolio detail: positions, valuation, history. Can be large — use pick (e.g. [\"portfolio.value\",\"positions.symbol\"]) or head to cap the position list."),
    patch("portfolios", "/api/portfolios/{id}", "Update portfolio settings.", schema::<crate::portfolios_api::UpdateBody>),
    delete("portfolios", "/api/portfolios/{id}", "Delete a portfolio."),
    post_empty("portfolios", "/api/portfolios/{id}/refresh", "Re-price the portfolio now."),
    post_empty("portfolios", "/api/portfolios/{id}/reconcile", "Reconcile positions against a broker statement."),
    post("portfolios", "/api/portfolios/{id}/assets", "Add an asset/position.", schema::<crate::portfolios_api::AddAssetBody>),
    get("portfolios", "/api/portfolios/assets/{asset_id}", "Asset detail with operations."),
    patch("portfolios", "/api/portfolios/assets/{asset_id}", "Update an asset.", schema::<crate::portfolios_api::PatchAssetBody>),
    delete("portfolios", "/api/portfolios/assets/{asset_id}", "Remove an asset."),
    post("portfolios", "/api/portfolios/assets/{asset_id}/operations", "Record a buy/sell/dividend operation.", schema::<crate::portfolios_api::AddOpBody>),
    delete("portfolios", "/api/portfolios/operations/{op_id}", "Delete an operation."),
    // ── backtest ─────────────────────────────────────────────────────────────
    // Read/run + strategy & custom-indicator authoring (creator surface).
    compute_post("backtest", "/api/backtest/run", "Run a backtest (strategy config + one or more datasets). Pass view=\"summary\": the default \"full\" also returns every trade and the whole equity curve, which overflows the response budget on a long dataset. Summary returns stats, per-asset breakdown, alignment and run_id. Every run is recorded in the history automatically; name it via POST /api/backtest/runs with that run_id to keep it permanently.\n\nUNITS inside `settings` — the two families do NOT share a scale, and the `_pct` suffix does not tell them apart. Get a worked example from GET /api/backtest/strategies/{id} before composing one by hand.\nFRACTIONS (0.01 = 1%): long/short.stop_loss_pct, long/short.take_profit_pct, stop_loss.value + take_profit.value when kind=\"pct\", slippage.value when kind=\"pct\", spread_pct, pyramiding min_distance_pct, oos_split_pct. A realistic crypto slippage is 0.0005 (=0.05%); 0.05 here means 5% per fill and will wipe the account.\nPERCENTS (10 = 10%): sizing.percent, sizing.risk_pct, sizing cap_pct, risk.max_exposure_pct, risk.max_exposure_per_asset_pct, risk.max_daily_loss_pct, risk.max_drawdown_pct, funding.annual_rate_pct. Also fees.amount when amount_kind=\"pct\". sizing.fraction (Kelly) is a true fraction (0.5 = half Kelly); equity_tiers value follows its `metric`.\nEvery run echoes the settings it used back in the report — check the echoed slippage/spread/sizing against what you intended before trusting a result.\n\nEXITS — `exit_on_reverse` means \"close as soon as the entry group stops holding\". With a crossover entry (crosses_above / crosses_below / cross) the group holds only on the bar of the cross, so the position is closed on the next bar and EVERY trade lasts one bar. Pair a crossover entry with an explicit `exit` group instead, and check `avg_bars_held` and `exit_reasons` in the result before believing any figure. The response carries a `warnings` array for exactly this kind of configuration — read it.\n\nDCA MODE — `kind`:\"dca\" is a savings plan, not a strategy, and it reads a `dca` object instead of `long`/`short`: `weights` (one row per ticker, any scale, normalised), `contribution` ({amount, period bar|day|week|month|quarter|year, every, invest}), `buys` and `sells`. `sizing`, `pyramiding`, `leverage`, `risk` and `oos_split_pct` are NOT read in this mode (a non-zero split is refused). A buy rule's `amount_kind` is fixed|pct_cash|pct_equity|pct_invested, a sell rule's is pct_position|all|units|amount, and an unknown value is refused rather than defaulted. `target_gain_pct` and every `pct_*` amount are PERCENTS (20 = 20%). `starting_capital` is bought at each asset's first bar; contributions and conditional tranches are new money on top, so `contributed` (not `starting_capital`) is the denominator of `return_pct`. Two operand families exist only here: `metric` (dd_from_high, up_from_low, change_pct, change_from_start) and `position` (pnl_pct, since_last_buy_pct, avg_cost, units, value, weight_pct, cash_pct, drawdown_pct). Read the `dca` block of the result, not the trade statistics: a plan that only sells at a profit has a 100% win rate by construction. Judge it on `twr_pct` (deposit-proof) and `irr_pct`, against `lump_sum_return_pct`.\n\nWINDOWING — `from`/`to` restrict the simulated span (\"YYYY-MM-DD\" or RFC3339; a plain end date covers that whole day). This is how you do walk-forward and regime slices: run the same settings over consecutive windows and compare. The window is recorded with the run, so a later report or Monte-Carlo replays the same span. Note `limit` keeps the MOST RECENT bars of the window, so leave it alone unless you mean to truncate.", schema::<crate::backtest_api::RunBody>),
    compute_post("backtest", "/api/backtest/sweep", "Run a parameter grid server-side and get EVERY trial back. Body: the same dataset/settings/from/to as /run, plus `grid` = {\"<dot.path.into.settings>\": [values…]} — e.g. {\"long.stop_loss_pct\": [0.01,0.02,0.03], \"sizing.percent\": [10,25]} is 6 trials. Max 4 axes, 64 trials. Units follow /api/backtest/run exactly; a grid value is written into settings verbatim, so a fraction stays a fraction.\n\nUse this instead of looping /run yourself. It is one tool round instead of N, and it makes the trial count part of the result rather than something you have to remember to mention.\n\nThe response is the trial table (params + stats + oos per trial, plus run_id when record=true), the spread of trial Sharpes, and `deflated_sharpe`: the Sharpe the BEST of N worthless strategies would be expected to reach, given how much these trials varied. When `selection_explains_it` is true the winner is inside the noise of having tried N times — report that as no evidence of an edge. Never present the best trial as expected performance; report the spread with it.", schema::<crate::backtest_api::SweepBody>),
    compute_post("backtest", "/api/backtest/align", "Multi-asset alignment preview (no simulation): merged-clock length, overlap window, warm-up bars, per-asset inactive bars. Takes the same from/to window as /run — pass it, or the preview describes a different span than the run it is warning you about.", schema::<crate::backtest_api::AlignBody>),
    get("backtest", "/api/backtest/runs", "Run history, newest first (every run lands here automatically). Query: filter=saved for the named/pinned runs only."),
    post("backtest", "/api/backtest/runs", "Name a run so it is kept permanently (pins it; never pruned by the history cap). Body: run_id (from /run) + name.", schema::<crate::backtest_api::SaveBody>),
    delete("backtest", "/api/backtest/runs", "Clear the auto history. Named/pinned runs are kept."),
    delete("backtest", "/api/backtest/runs/{id}", "Delete one run from the history."),
    compute_post("backtest", "/api/backtest/runs/{id}/montecarlo", "Monte-Carlo resampling of a saved run's realized per-trade P&L: many equity paths drawn from the trades the run actually made, giving percentile bands for final equity and max drawdown, plus risk of ruin and probability of loss. Pass view=\"summary\" — the default \"full\" also returns the per-step equity fan, two histograms and the realized curve, which exist to draw a chart and will overflow the response budget.\n\nIt answers \"how else could this sequence of trades have gone\", NOT \"is the edge real\": it resamples the trades you already have, so a fitted strategy yields confident-looking bands around a fitted result. Needs at least 2 trades. Useful for sizing and ruin risk; not evidence of an edge.", schema::<crate::backtest_api::MonteCarloBody>),
    get("backtest", "/api/backtest/runs/{id}/report.md", "Markdown report for a saved run (front matter, stats table, human-readable settings, exit reasons)."),
    get("backtest", "/api/backtest/strategies", "List saved strategies (named Settings)."),
    post("backtest", "/api/backtest/strategies", "Create a strategy (named engine Settings; validated). Returns id.", schema::<crate::backtest_api::StrategyBody>),
    get("backtest", "/api/backtest/strategies/{id}", "Get one strategy with its full settings (feed straight to /run)."),
    put("backtest", "/api/backtest/strategies/{id}", "Update a strategy.", schema::<crate::backtest_api::StrategyBody>),
    delete("backtest", "/api/backtest/strategies/{id}", "Delete a strategy."),
    // Optimizer: the grid as a job. `/sweep` (64 trials, one response) is still the right
    // tool for a quick question; this is for the five-figure grids the screen runs.
    compute_post("backtest", "/api/backtest/optimize/params", "Which parameters of a strategy a grid can vary: every settings path that exists, the value it holds today, its unit and a suggested sweep. Body: strategy_id (from /api/backtest/strategies) or settings. START HERE. An axis path is not validated against the settings: a path with a typo is written in as a dead key, and the grid then runs the base strategy N times and reports it as a result.", schema::<crate::backtest_optimize_api::ParamsBody>),
    compute_post("backtest", "/api/backtest/optimize/estimate", "Count the variants and time one of them, for real: it loads the bars and runs variant 0, so \"43 minutes\" comes from this machine and this strategy. Writes nothing, starts nothing. Same body as /optimize. Call it before /optimize and tell the user the number.", schema::<crate::backtest_optimize_api::GridBody>),
    post("backtest", "/api/backtest/optimize", "Start the grid; returns a job id. This takes every core for minutes: run /optimize/estimate first and let the user approve the wait. One job at a time (a second call is refused while one runs). Body: dataset_ids + settings + axes (paths from /optimize/params, values in engine units) + optional from/to/limit/metric.", schema::<crate::backtest_optimize_api::GridBody>),
    get("backtest", "/api/backtest/optimize", "Optimizer jobs held in memory (id, state, progress). Nothing is persisted: a finished job is gone at the next restart, so keep a result by re-running its variant through /api/backtest/run and naming it."),
    get("backtest", "/api/backtest/optimize/{id}", "Progress plus one page of the ranking. Query: sort (metric), dir, offset, limit (max 500), analysis=1 for the per-axis sensitivity. The full grid is never serialized: page it."),
    post_empty("backtest", "/api/backtest/optimize/{id}/cancel", "Stop a running job. What it already computed stays readable."),
    delete("backtest", "/api/backtest/optimize/{id}", "Forget a job and its rows."),
    get("backtest", "/api/backtest/indicators", "List saved custom indicators (node-graph definitions)."),
    post("backtest", "/api/backtest/indicators", "Create a custom indicator (node-graph DAG JSON; validated: bounded, no forward/self refs). Returns id.", schema::<crate::backtest_api::IndicatorBody>),
    get("backtest", "/api/backtest/indicators/{id}", "Get one custom-indicator definition."),
    put("backtest", "/api/backtest/indicators/{id}", "Update a custom indicator.", schema::<crate::backtest_api::IndicatorBody>),
    delete("backtest", "/api/backtest/indicators/{id}", "Delete a custom indicator."),
    // ── quant ────────────────────────────────────────────────────────────────
    compute_post("quant", "/api/quant/single", "Risk/return metrics for one asset's series (VaR, CVaR, vol, drawdown). Response includes per-bar drawdown_curve + histogram for charting — pass pick (e.g. [\"result.var_hist\",\"result.cvar\"]) when the user only needs figures.", schema::<crate::quant_api::SingleBody>),
    compute_post("quant", "/api/quant/kelly", "Kelly criterion sizing.", schema::<crate::quant_api::KellyBody>),
    compute_post("quant", "/api/quant/size", "Position sizing calculator.", schema::<crate::quant_api::SizeBody>),
    compute_post("quant", "/api/quant/asset-signals", "Technical signals for an asset.", schema::<crate::quant_api::AssetSignalsBody>),
    compute_post("quant", "/api/quant/seasonality", "Calendar breakdown of an asset's returns (by month, weekday and hour where the timeframe allows), with the sample size behind each bucket. Read the counts before the averages: a \"best month\" drawn from four observations is noise.", schema::<crate::quant_api::SeasonalityBody>),
    compute_post("quant", "/api/quant/portfolio", "Portfolio-level metrics (correlations, vol, drawdown…). Large response — pick the metric fields the user asked for.", schema::<crate::quant_api::PortfolioBody>),
    // ── histdata ─────────────────────────────────────────────────────────────
    get("histdata", "/api/histdata/datasets", "List downloaded datasets."),
    get("histdata", "/api/histdata/datasets/{id}/bars", "OHLCV bars. Query: from, to, limit, offset."),
    post("histdata", "/api/histdata/downloads", "Queue a new dataset download. Check /api/connectors/providers for valid provider/asset_type/timeframe combos first.", schema::<crate::histdata_api::DownloadBody>),
    compute_post("histdata", "/api/histdata/preview", "Fetch a trailing window of bars straight from a provider WITHOUT storing anything (default 500 bars, max 5000). Use it to look at an instrument that was never downloaded — a quick quote, a chart, a check that a symbol is spelled the way the provider wants. The response also reports whether a dataset for these coordinates already exists. To keep the data, call /api/histdata/preview/save.", schema::<crate::histdata_api::PreviewBody>),
    post("histdata", "/api/histdata/preview/save", "Store the window a preview showed: queues the normal download job for those coordinates, consolidating with any bars already stored. Either an explicit from/to window or a trailing `bars` count.", schema::<crate::histdata_api::SavePreviewBody>),
    get("histdata", "/api/histdata/symbols", "Look instruments up across the connectors granted to the chart. Query: q (symbol or name), asset_type, connectors (comma-separated connector ids), limit. Returns each hit's provider-native symbol plus `notes` for connectors that could not answer (no search endpoint, missing key, rate limit)."),
    compute_post("histdata", "/api/histviz/series", "One window of an instrument for charting: bars already stored are read from the catalog and only the missing edges are fetched through the connector. Stores nothing. Body: coordinates + `to` (RFC3339 exclusive upper bound, default now) + `bars` (slice size, default 1500). Walk backwards through history by passing the previous slice's oldest timestamp as `to`. `history_start` means the provider serves nothing older; `notice` carries the reason when a window came back short (auth, quota, rate_limit, depth, symbol).", schema::<crate::histdata_api::SeriesBody>),
    post_empty("histdata", "/api/histdata/datasets/{id}/append", "Extend a dataset with newer bars (from its last bar to now)."),
    get("histdata", "/api/histdata/jobs", "Download queue/job status."),
    delete("histdata", "/api/histdata/jobs/{id}", "Cancel one queued or running download."),
    delete("histdata", "/api/histdata/jobs/batch/{id}", "Cancel every job of a batch (one row per symbol of a multi-symbol download)."),
    get("histdata", "/api/histdata/datasets/{id}/export", "The dataset's bars as CSV text. Large: /datasets/{id}/bars with from/to/limit is the normal read."),
    delete("histdata", "/api/histdata/datasets/{id}", "Delete a dataset."),
    // ── connectors (data broker) ─────────────────────────────────────────────
    // Read-only on purpose: creating connectors and setting credentials handles secrets,
    // which never goes through MCP.
    get("connectors", "/api/connectors/providers", "Available market-data providers and their capabilities (valid asset_type/timeframe combos per provider, which need an API key, which can stream live)."),
    get("connectors", "/api/connectors", "Configured connectors (named provider accounts) with the modules they are granted to, which credentials are set (names only, never values) and their request-quota usage. Query: module=histdata|watchlists|histviz to see only what that module may use."),
    // ── findb ────────────────────────────────────────────────────────────────
    get("findb", "/api/findb/status", "FinanceDatabase install status."),
    get("findb", "/api/findb/search", "Search financial products. Query: q, kind, exchange, country, sector, limit, offset."),
    get("findb", "/api/findb/facets", "Distinct values of ONE column, for building a filter. Query: column (REQUIRED, one of asset_type|exchange|currency|country|sector|industry|category|family), type (optional asset_type filter)."),
    get("findb", "/api/findb/folders", "List favorite folders."),
    post("findb", "/api/findb/folders", "Create a folder.", schema::<otw_store::findb::FolderInput>),
    patch("findb", "/api/findb/folders/{id}", "Rename a folder.", schema::<otw_store::findb::FolderInput>),
    delete("findb", "/api/findb/folders/{id}", "Delete a folder."),
    get("findb", "/api/findb/favorites", "List favorite products."),
    post("findb", "/api/findb/favorites", "Add a favorite.", schema::<otw_store::findb::FavoriteInput>),
    patch("findb", "/api/findb/favorites/{id}", "Move/annotate a favorite.", schema::<otw_store::findb::FavoriteInput>),
    delete("findb", "/api/findb/favorites/{id}", "Remove a favorite."),
    // ── watchlists ───────────────────────────────────────────────────────────
    get("watchlists", "/api/watchlists", "List watchlists with item counts."),
    get("watchlists", "/api/watchlists/{id}", "One watchlist with its items and cached quotes (price, 24h/3d/7d/30d changes)."),
    get("watchlists", "/api/watchlists/search", "Resolve a symbol to add. Query: q, kind (crypto|stock)."),
    get("watchlists", "/api/watchlists/templates", "Starter templates (id, name, symbols) usable when creating a watchlist."),
    post("watchlists", "/api/watchlists", "Create a watchlist (optionally from a starter template).", schema::<crate::watchlists_api::CreateBody>),
    patch("watchlists", "/api/watchlists/{id}", "Update a watchlist (name, description, sync, refresh cadence, quote source).", schema::<crate::watchlists_api::UpdateBody>),
    delete("watchlists", "/api/watchlists/{id}", "Delete a watchlist and its items."),
    post("watchlists", "/api/watchlists/{id}/items", "Add a symbol.", schema::<crate::watchlists_api::AddItemBody>),
    patch("watchlists", "/api/watchlists/items/{item_id}", "Update an item (notes, position, quote overrides).", schema::<crate::watchlists_api::PatchItemBody>),
    delete("watchlists", "/api/watchlists/items/{item_id}", "Remove an item."),
    post_empty("watchlists", "/api/watchlists/{id}/refresh", "Re-quote every item now."),
    post("watchlists", "/api/watchlists/{id}/import", "Copy a Portfolio Tracker portfolio's assets onto the watchlist. Upserts: symbols already there are refreshed, not duplicated.", schema::<crate::watchlists_api::ImportBody>),
    post("watchlists", "/api/watchlists/items/{item_id}/alerts", "Create a price alert on an item: metric + direction + threshold, optional basis/window/repeat/cooldown, and the notification channels to push to. A channel the Watchlists module is not granted is refused, so read /api/notif-channels first (that endpoint is not on this gateway: ask the user for the channel ids).", schema::<otw_store::watchlist_alerts::AlertInput>),
    patch("watchlists", "/api/watchlists/alerts/{alert_id}", "Update an alert (same full payload as create).", schema::<otw_store::watchlist_alerts::AlertInput>),
    delete("watchlists", "/api/watchlists/alerts/{alert_id}", "Delete an alert."),
    // ── mportfolios ──────────────────────────────────────────────────────────
    get("mportfolios", "/api/mportfolios", "List famous managers' portfolios (Dataroma cache)."),
    get("mportfolios", "/api/mportfolios/{slug}", "One manager's holdings."),
    post_empty("mportfolios", "/api/mportfolios/refresh", "Refresh the cache from source."),
    get("mportfolios", "/api/mportfolios/snapshots", "List saved snapshots."),
    post("mportfolios", "/api/mportfolios/snapshots", "Snapshot current holdings for comparison.", schema::<crate::mportfolios_api::SnapshotBody>),
    get("mportfolios", "/api/mportfolios/snapshots/{id}", "Snapshot detail."),
    delete("mportfolios", "/api/mportfolios/snapshots/{id}", "Delete a snapshot."),
    delete("mportfolios", "/api/mportfolios/snapshots/by-slug/{slug}", "Delete every snapshot of one manager."),
    // ── wealth ───────────────────────────────────────────────────────────────
    get("wealth", "/api/wealth/assets", "List wealth assets (accounts, property, holdings…)."),
    post("wealth", "/api/wealth/assets", "Add an asset.", schema::<otw_store::wealth::AssetInput>),
    patch("wealth", "/api/wealth/assets/{id}", "Update an asset.", schema::<otw_store::wealth::AssetInput>),
    delete("wealth", "/api/wealth/assets/{id}", "Delete an asset."),
    get("wealth", "/api/wealth/assets/{id}/revisions", "Value history of an asset."),
    post("wealth", "/api/wealth/assets/{id}/revisions", "Record a new valuation.", schema::<otw_store::wealth::RevisionInput>),
    patch("wealth", "/api/wealth/revisions/{id}", "Update a valuation.", schema::<otw_store::wealth::RevisionInput>),
    delete("wealth", "/api/wealth/revisions/{id}", "Delete a valuation."),
    get("wealth", "/api/wealth/breakdown", "Net-worth breakdown over time."),
    get("wealth", "/api/wealth/templates", "List asset templates."),
    post("wealth", "/api/wealth/templates", "Create an asset template.", schema::<otw_store::wealth::TemplateInput>),
    patch("wealth", "/api/wealth/templates/{id}", "Update a template.", schema::<otw_store::wealth::TemplatePatch>),
    delete("wealth", "/api/wealth/templates/{id}", "Delete a template."),
    get("wealth", "/api/wealth/settings", "Wealth settings."),
    patch("wealth", "/api/wealth/settings", "Update wealth settings.", schema::<crate::wealth_api::SettingsBody>),
    // ── subscriptions ────────────────────────────────────────────────────────
    get("subscriptions", "/api/subscriptions", "List recurring subscriptions."),
    post("subscriptions", "/api/subscriptions", "Add a subscription.", schema::<otw_store::subscriptions::SubscriptionInput>),
    get("subscriptions", "/api/subscriptions/{id}", "Subscription detail."),
    patch("subscriptions", "/api/subscriptions/{id}", "Update a subscription.", schema::<otw_store::subscriptions::SubscriptionInput>),
    delete("subscriptions", "/api/subscriptions/{id}", "Delete a subscription."),
    get("subscriptions", "/api/subscriptions/breakdown", "Cost breakdown (monthly/yearly, by category)."),
    get("subscriptions", "/api/subscriptions/suggestions", "Known services the user could be paying for, to seed a subscription."),
    get("subscriptions", "/api/subscriptions/settings", "Subscription settings."),
    patch("subscriptions", "/api/subscriptions/settings", "Update settings.", schema::<crate::subscriptions_api::SettingsBody>),
    // ── taxcalc ──────────────────────────────────────────────────────────────
    get("taxcalc", "/api/taxcalc/templates", "Available country tax templates."),
    get("taxcalc", "/api/taxcalc/profiles", "List tax profiles."),
    post("taxcalc", "/api/taxcalc/profiles", "Create a profile.", schema::<crate::taxcalc_api::ProfileBody>),
    get("taxcalc", "/api/taxcalc/profiles/{id}", "Profile detail."),
    put("taxcalc", "/api/taxcalc/profiles/{id}", "Update a profile.", schema::<crate::taxcalc_api::ProfileBody>),
    delete("taxcalc", "/api/taxcalc/profiles/{id}", "Delete a profile."),
    compute_post("taxcalc", "/api/taxcalc/compute", "Stateless tax computation from inputs.", schema::<crate::taxcalc_api::ScenarioBody>),
    compute_post("taxcalc", "/api/taxcalc/scenarios/{id}/compute", "Run the engine against a profile and scenario inputs and return the result. Persists nothing: saving to history is POST /scenarios. The {id} in the path is ignored, the body carries everything.", schema::<crate::taxcalc_api::ScenarioBody>),
    get("taxcalc", "/api/taxcalc/scenarios", "List saved scenarios."),
    post("taxcalc", "/api/taxcalc/scenarios", "Create a scenario.", schema::<crate::taxcalc_api::ScenarioBody>),
    get("taxcalc", "/api/taxcalc/scenarios/{id}", "Scenario detail."),
    put("taxcalc", "/api/taxcalc/scenarios/{id}", "Update a scenario.", schema::<crate::taxcalc_api::ScenarioBody>),
    delete("taxcalc", "/api/taxcalc/scenarios/{id}", "Delete a scenario."),
    compute_post_empty("taxcalc", "/api/taxcalc/scenarios/{id}/compute", "Compute a saved scenario."),
    // ── editor ───────────────────────────────────────────────────────────────
    get("editor", "/api/documents", "List documents (tree)."),
    post("editor", "/api/documents", "Create a document.", schema::<crate::documents::CreateBody>),
    get("editor", "/api/documents/{id}", "Document content (rich-text blocks JSON)."),
    patch("editor", "/api/documents/{id}", "Update title/content.", schema::<crate::documents::UpdateBody>),
    delete("editor", "/api/documents/{id}", "Delete a document."),
    post("editor", "/api/documents/{id}/move", "Move in the tree.", schema::<crate::documents::MoveBody>),
    post("editor", "/api/documents/{id}/flag", "Set or clear a document's colour flag in the tree.", schema::<crate::documents::FlagBody>),
    get("editor", "/api/databases/{id}", "Load an editor database (columns + rows)."),
    post("editor", "/api/databases/{id}/columns", "Add a column.", schema::<crate::databases::AddColumnBody>),
    patch("editor", "/api/databases/columns/{col_id}", "Update a column.", schema::<otw_store::databases::ColumnPatch>),
    delete("editor", "/api/databases/columns/{col_id}", "Delete a column."),
    post("editor", "/api/databases/{id}/rows", "Add a row.", schema::<crate::databases::AddRowBody>),
    patch("editor", "/api/databases/rows/{row_id}", "Update a row.", schema::<crate::databases::UpdateRowBody>),
    delete("editor", "/api/databases/rows/{row_id}", "Delete a row."),
    post("editor", "/api/databases/rows/{row_id}/move", "Reorder a row.", schema::<crate::databases::MoveRowBody>),
    // ── todos ────────────────────────────────────────────────────────────────
    get("todos", "/api/todos", "List todos."),
    post("todos", "/api/todos", "Add a todo.", schema::<otw_store::todos::TodoInput>),
    get("todos", "/api/todos/{id}", "Todo detail."),
    patch("todos", "/api/todos/{id}", "Update a todo.", schema::<otw_store::todos::TodoInput>),
    delete("todos", "/api/todos/{id}", "Delete a todo."),
    patch("todos", "/api/todos/{id}/done", "Toggle done.", schema::<crate::todos_api::DoneBody>),
    // ── goals ────────────────────────────────────────────────────────────────
    get("goals", "/api/goals", "List goals with progress."),
    post("goals", "/api/goals", "Create a goal.", schema::<otw_store::goals::GoalInput>),
    get("goals", "/api/goals/{id}", "Goal detail."),
    patch("goals", "/api/goals/{id}", "Update a goal / progress.", schema::<otw_store::goals::GoalInput>),
    delete("goals", "/api/goals/{id}", "Delete a goal."),
    post("goals", "/api/goals/{id}/position", "Reorder a goal in the list.", schema::<crate::goals_api::PositionBody>),
    // ── remindme ─────────────────────────────────────────────────────────────
    get("remindme", "/api/reminders", "List reminders."),
    post("remindme", "/api/reminders", "Create a reminder (one-shot or recurring).", schema::<otw_store::reminders::ReminderInput>),
    get("remindme", "/api/reminders/{id}", "Reminder detail."),
    patch("remindme", "/api/reminders/{id}", "Update a reminder.", schema::<otw_store::reminders::ReminderInput>),
    delete("remindme", "/api/reminders/{id}", "Delete a reminder."),
    get("remindme", "/api/notifications", "In-app notifications."),
    get("remindme", "/api/notifications/unread", "Unread count."),
    post_empty("remindme", "/api/notifications/ack-all", "Mark all read."),
    post_empty("remindme", "/api/notifications/{id}/read", "Mark one read."),
    delete("remindme", "/api/notifications/{id}", "Delete one notification."),
    // ── calendar ─────────────────────────────────────────────────────────────
    get("calendar", "/api/calendar/events", "List events. Query: from, to."),
    post("calendar", "/api/calendar/events", "Create an event.", schema::<otw_store::calendar::CalendarEventInput>),
    get("calendar", "/api/calendar/events/{id}", "Event detail."),
    patch("calendar", "/api/calendar/events/{id}", "Update an event.", schema::<otw_store::calendar::CalendarEventInput>),
    delete("calendar", "/api/calendar/events/{id}", "Delete an event."),
    // ── time ─────────────────────────────────────────────────────────────────
    get("time", "/api/time/projects", "List time-tracking projects."),
    post("time", "/api/time/projects", "Create a project.", schema::<otw_store::time_tracker::ProjectInput>),
    get("time", "/api/time/projects/{id}", "Project detail."),
    patch("time", "/api/time/projects/{id}", "Update a project.", schema::<otw_store::time_tracker::ProjectInput>),
    delete("time", "/api/time/projects/{id}", "Delete a project."),
    post_empty("time", "/api/time/projects/{id}/start", "Start the timer on a project."),
    post_empty("time", "/api/time/projects/{id}/stop", "Stop the timer."),
    get("time", "/api/time/projects/{id}/entries", "List entries for a project."),
    post("time", "/api/time/projects/{id}/entries", "Add a manual entry.", schema::<crate::time_api::CreateEntryBody>),
    delete("time", "/api/time/entries/{id}", "Delete an entry."),
    post("time", "/api/time/projects/{id}/position", "Reorder a project.", schema::<crate::time_api::PositionBody>),
    post_empty("time", "/api/time/heartbeat", "Keep the running timer alive (the UI beats while the tab is open)."),
    post_empty("time", "/api/time/revert", "Undo the last start/stop."),
    patch("time", "/api/time/settings", "Update time-tracker settings (hourly cost, rounding…).", schema::<crate::time_api::SettingsBody>),
    get("time", "/api/time/state", "Current running timer, if any."),
    get("time", "/api/time/breakdown", "Tracked-time breakdown."),
    // ── routines (trader board) ──────────────────────────────────────────────
    get("routines", "/api/trader/board", "Today's trader board: routines + tasks with check state."),
    get("routines", "/api/trader/routines", "List routine templates with their checklist items."),
    get("routines", "/api/trader/categories", "List routine categories."),
    post("routines", "/api/trader/routines", "Create a routine template.", schema::<crate::trader_tasks_api::CreateRoutineBody>),
    get("routines", "/api/trader/routines/{id}", "One routine template with its items and schedule."),
    patch("routines", "/api/trader/routines/{id}", "Update a routine. `items` is a full replacement list when present; existing items keep their id so past checks stay attached.", schema::<crate::trader_tasks_api::UpdateRoutineBody>),
    delete("routines", "/api/trader/routines/{id}", "Delete a routine and its items."),
    post("routines", "/api/trader/routines/{id}/duplicate", "Copy a routine (optionally under a new name).", schema::<crate::trader_tasks_api::DuplicateBody>),
    post("routines", "/api/trader/routines/reorder", "Reorder the routine list.", schema::<crate::trader_tasks_api::ReorderBody>),
    post("routines", "/api/trader/categories", "Create a routine category.", schema::<crate::trader_tasks_api::CategoryBody>),
    patch("routines", "/api/trader/categories/{id}", "Update a category.", schema::<crate::trader_tasks_api::CategoryPatch>),
    delete("routines", "/api/trader/categories/{id}", "Delete a category (its routines stay, uncategorized)."),
    post("routines", "/api/trader/items/{id}/check", "Check/uncheck a routine item today.", schema::<crate::trader_tasks_api::CheckBody>),
    get("routines", "/api/trader/marks", "Consistency day marks. Query: from, to."),
    put("routines", "/api/trader/marks", "Mark a day full|action, or null to clear.", schema::<crate::trader_tasks_api::SetMarkBody>),
    post("routines", "/api/trader/tasks", "Add a one-off task.", schema::<crate::trader_tasks_api::AddTaskBody>),
    patch("routines", "/api/trader/tasks/{id}", "Update a task.", schema::<crate::trader_tasks_api::UpdateTaskBody>),
    delete("routines", "/api/trader/tasks/{id}", "Delete a task."),
    // ── mindset ──────────────────────────────────────────────────────────────
    get("mindset", "/api/mindset/day", "Today's check-in templates, prompts + entries. Query: date."),
    put("mindset", "/api/mindset/entries", "Save a check-in for a template/date.", schema::<crate::mindset_api::SaveEntryBody>),
    get("mindset", "/api/mindset/history", "Past entries."),
    get("mindset", "/api/mindset/marks", "Consistency day marks. Query: from, to."),
    put("mindset", "/api/mindset/marks", "Mark a day full|action, or null to clear.", schema::<crate::mindset_api::SetMarkBody>),
    delete_body("mindset", "/api/mindset/entries", "Delete a saved check-in. Body names the target: date, plus template_id or phase.", schema::<crate::mindset_api::DeleteEntryBody>),
    get("mindset", "/api/mindset/templates", "List check-in templates with their prompts."),
    post("mindset", "/api/mindset/templates", "Create a check-in template.", schema::<crate::mindset_api::CreateTemplateBody>),
    delete("mindset", "/api/mindset/templates", "Delete EVERY template (and with them their prompts). Destructive: confirm with the user first."),
    post_empty("mindset", "/api/mindset/templates/reset", "Restore the built-in starter templates."),
    get("mindset", "/api/mindset/templates/{id}", "One template with its prompts."),
    patch("mindset", "/api/mindset/templates/{id}", "Update a template. `prompts` is a full replacement list when present; entries keep their id so past answers stay attached.", schema::<crate::mindset_api::UpdateTemplateBody>),
    delete("mindset", "/api/mindset/templates/{id}", "Delete one template."),
    post("mindset", "/api/mindset/templates/{id}/duplicate", "Copy a template (optionally under a new name).", schema::<crate::mindset_api::DuplicateBody>),
    get("mindset", "/api/mindset/categories", "List check-in categories."),
    post("mindset", "/api/mindset/categories", "Create a check-in category.", schema::<crate::mindset_api::CategoryBody>),
    patch("mindset", "/api/mindset/categories/{id}", "Update a category.", schema::<crate::mindset_api::CategoryPatch>),
    delete("mindset", "/api/mindset/categories/{id}", "Delete a category (its templates stay, uncategorized)."),
    get("mindset", "/api/mindset/prompts", "List prompts."),
    post("mindset", "/api/mindset/prompts", "Add a prompt.", schema::<crate::mindset_api::AddPromptBody>),
    patch("mindset", "/api/mindset/prompts/{id}", "Update a prompt.", schema::<crate::mindset_api::UpdatePromptBody>),
    delete("mindset", "/api/mindset/prompts/{id}", "Delete a prompt."),
    // ── news ─────────────────────────────────────────────────────────────────
    get("news", "/api/feeds", "List configured feeds."),
    post("news", "/api/feeds", "Add a feed.", schema::<crate::feeds_api::CreateFeed>),
    get("news", "/api/feeds/{id}", "Feed detail."),
    patch("news", "/api/feeds/{id}", "Update a feed.", schema::<otw_store::feeds::FeedPatch>),
    delete("news", "/api/feeds/{id}", "Delete a feed."),
    post_empty("news", "/api/feeds/{id}/refresh", "Poll one feed now."),
    post_empty("news", "/api/feeds/refresh-all", "Poll all feeds now."),
    get("news", "/api/feed-items", "Aggregated items. Query: feed_id, search, limit, offset."),
    get("news", "/api/feed-sources", "Known source templates."),
    get("news", "/api/feeds/quotas", "Per-feed request quota usage."),
    get("news", "/api/feed-dashboards", "List feed dashboards (named subsets of the feeds, one of them the default)."),
    post("news", "/api/feed-dashboards", "Create a dashboard.", schema::<crate::feeds_api::CreateDashboard>),
    get("news", "/api/feed-dashboards/{id}", "One dashboard with its items."),
    patch("news", "/api/feed-dashboards/{id}", "Update a dashboard (name, favorite, position, started).", schema::<otw_store::feeds::DashboardPatch>),
    delete("news", "/api/feed-dashboards/{id}", "Delete a dashboard (its feeds stay)."),
    post_empty("news", "/api/feed-dashboards/{id}/default", "Make it the dashboard the module opens on."),
    post_empty("news", "/api/feed-dashboards/{id}/refresh", "Poll every feed of this dashboard now."),
    get("news", "/api/feed-dashboards/{id}/sources", "Feeds on this dashboard."),
    post("news", "/api/feed-dashboards/{id}/sources", "Add a feed to the dashboard.", schema::<crate::feeds_api::AddSource>),
    delete("news", "/api/feed-dashboards/{id}/sources/{feed_id}", "Remove a feed from the dashboard."),
    // ── resources ────────────────────────────────────────────────────────────
    get("resources", "/api/resources", "List saved resources (links/notes)."),
    post("resources", "/api/resources", "Add a resource.", schema::<otw_store::resources::ResourceInput>),
    patch("resources", "/api/resources/{id}", "Update a resource.", schema::<otw_store::resources::ResourceInput>),
    delete("resources", "/api/resources/{id}", "Delete a resource."),
    get("resources", "/api/resources/categories", "List resource categories."),
    post("resources", "/api/resources/categories", "Add a category.", schema::<otw_store::resources::CategoryInput>),
    patch("resources", "/api/resources/categories/{id}", "Update a category.", schema::<otw_store::resources::CategoryInput>),
    delete("resources", "/api/resources/categories/{id}", "Delete a category."),
    // ── prompt-store ─────────────────────────────────────────────────────────
    get("prompt-store", "/api/prompts", "List saved prompts (name, body, tags)."),
    post("prompt-store", "/api/prompts", "Create a prompt.", schema::<otw_store::prompts::PromptInput>),
    get("prompt-store", "/api/prompts/{id}", "Get one prompt."),
    patch("prompt-store", "/api/prompts/{id}", "Edit a prompt. Every save appends a version; nothing is overwritten in place.", schema::<otw_store::prompts::PromptInput>),
    delete("prompt-store", "/api/prompts/{id}", "Delete a prompt and its version history."),
    get("prompt-store", "/api/prompts/tags", "Every tag in use, for filtering."),
    get("prompt-store", "/api/prompts/{id}/versions", "Version history of a prompt, newest first."),
    post("prompt-store", "/api/prompts/{id}/rollback", "Restore an old version. It is appended as a new version, so the history stays append-only.", schema::<crate::prompts_api::RollbackBody>),
    post_empty("prompt-store", "/api/prompts/{id}/duplicate", "Copy a prompt."),
    patch("prompt-store", "/api/prompts/{id}/vote", "Set the thumbs up/down flag the quick filter reads.", schema::<crate::prompts_api::VoteBody>),
    // ── mailbox ──────────────────────────────────────────────────────────────
    // Read-only over stored mail, plus the curated newsletter store. Account
    // management, settings, attachments and unsubscribe stay out: they handle
    // credentials or reach the network on the user's behalf.
    get("mailbox", "/api/mailbox/senders", "List mail senders with category, status and unread counts."),
    get("mailbox", "/api/mailbox/messages", "List stored mail. Query: sender_id, account_id, category, q, unread, starred, archived, limit, offset."),
    get("mailbox", "/api/mailbox/messages/{id}", "Read one message (sanitised body + attachment list)."),
    patch("mailbox", "/api/mailbox/messages/{id}", "Set read / starred / archived on one message. Local state only: no IMAP flag is ever written (the connection is read-only).", schema::<otw_store::mailbox::MessagePatch>),
    delete("mailbox", "/api/mailbox/messages/{id}", "Delete a stored message locally (the mail stays on the server)."),
    post("mailbox", "/api/mailbox/messages/read-all", "Mark every stored message read, or just one sender's with sender_id.", schema::<crate::mailbox_api::ReadAllBody>),
    post("mailbox", "/api/mailbox/messages/{id}/remind", "Raise a reminder about this message at a given time.", schema::<crate::mailbox_api::RemindBody>),
    get("mailbox", "/api/mailbox/links", "List the curated newsletter store."),
    post("mailbox", "/api/mailbox/links", "Add a newsletter to the store.", schema::<otw_store::mailbox::StoreLinkInput>),
    patch("mailbox", "/api/mailbox/links/{id}", "Edit a store entry.", schema::<otw_store::mailbox::StoreLinkPatch>),
    delete("mailbox", "/api/mailbox/links/{id}", "Remove a store entry."),

    // ── automator ────────────────────────────────────────────────────────────
    // Authoring only. A graph written here is a PROPOSAL: it is stored as the workflow's
    // draft, the editor shows it, and it starts running when its owner saves it. There is
    // deliberately no way from this gateway to run a workflow, attach its access token,
    // restore a revision, delete one, or touch a schedule.
    get("automator", "/api/automator/workflows", "List workflows with their schedules and last run."),
    get("automator", "/api/automator/workflows/{id}", "One workflow: its graph, its draft (an unadopted proposal), its schedules and recent runs. Read this before editing: the graph you save replaces the draft wholesale."),
    get("automator", "/api/automator/workflows/{id}/versions", "Revision history of a workflow's graph."),
    post("automator", "/api/automator/workflows", "Create an empty workflow, then fill it with PUT /api/automator/workflows/{id}.", schema::<crate::automator_api::CreateInput>),
    patch("automator", "/api/automator/workflows/{id}", "Rename a workflow or edit its description, favorite, enabled flag and run limit. `mcp_token_id` is refused here: a workflow's access token is attached by its owner, in the editor.", schema::<crate::automator_api::PatchInput>),
    put("automator", "/api/automator/workflows/{id}", "Save a graph as the workflow's DRAFT (it does not become the running graph: its owner adopts it from the editor). An invalid graph is refused with the reason, so fix and re-save. Read GET /api/automator/catalog first for the block kinds and the endpoints an `api` block may call. `repoint_schedules` and `draft` in the body are ignored here.", schema::<crate::automator_api::SaveInput>),
    compute_post("automator", "/api/automator/workflows/{id}/test", "Test-run the workflow's draft (its graph if there is no draft) and return the whole trace, block by block. Notifications and external calls are forced off and an `api` block has no envelope, so nothing leaves the machine and nothing is written: this checks the topology, the expressions and the transforms. Refused on a workflow that already carries an access token, which its owner tests from its own page.", schema::<crate::automator_api::TestInput>),
    get("automator", "/api/automator/catalog", "The block palette: the block kinds, the operations a `transform` and an `if` accept, and every app endpoint an `api` block may call. Read it before composing a graph."),
    get("automator", "/api/automator/catalog/schema", "Request-body JSON Schema of ONE endpoint an `api` block would call. Query: method and path, both REQUIRED (e.g. method=POST&path=/api/reminders)."),
    get("automator", "/api/automator/runs", "Run history. Query: workflow_id, status, limit."),
    get("automator", "/api/automator/runs/{id}", "One run with its per-block trace: request, output, status and timing."),

    // ── search ───────────────────────────────────────────────────────────────
    get("search", "/api/search", "Title-level search across the modules that store text (documents, trades, notes, resources, prompts…). Query: q (REQUIRED), scopes (comma-separated, default all). Returns titles and ids, never bodies: read the hit through its own module endpoint. NOTE for whoever grants this: it does not check the token's other module keys, so it reads titles everywhere."),

    // ── community-docs ───────────────────────────────────────────────────────
    get("community-docs", "/api/community-docs", "List community docs."),
    get("community-docs", "/api/community-docs/{slug}", "Read one doc."),
    get("community-docs", "/api/community-docs/favorites", "List favorites."),
    put("community-docs", "/api/community-docs/{slug}/favorite", "Set/unset favorite.", schema::<crate::community_docs_api::FavoriteBody>),
    post_empty("community-docs", "/api/community-docs/refresh", "Refresh the catalog from the website."),
];

/// True when `path` (no query string) matches the `/`-segmented template, where a
/// `{param}` segment matches any single non-empty segment.
pub fn path_matches(template: &str, path: &str) -> bool {
    let t: Vec<&str> = template.split('/').collect();
    let p: Vec<&str> = path.split('/').collect();
    t.len() == p.len()
        && t.iter().zip(&p).all(|(ts, ps)| {
            (ts.starts_with('{') && ts.ends_with('}') && !ps.is_empty()) || ts == ps
        })
}

/// Find the allowlist entry for a concrete request, if any.
///
/// A literal segment beats a `{param}` one, the way Axum itself routes: `/api/watchlists/{id}`
/// is declared before `/api/watchlists/search` and matches it, so a first-match scan would hand
/// back the wrong entry. Today every such pair shares a module and a `compute` flag, so nothing
/// observable changes — but the entry returned here decides the permission check and whether a
/// write asks the user first, and neither may hinge on catalog ordering.
pub fn lookup(method: &str, path: &str) -> Option<&'static Endpoint> {
    let of_method = || CATALOG.iter().filter(move |e| e.method == method);
    of_method()
        .find(|e| e.path == path)
        .or_else(|| of_method().find(|e| path_matches(e.path, path)))
}

/// Methods allowed on `path`, whatever the method asked for. Used to turn a
/// right-path/wrong-method miss into a message that names the fix instead of claiming
/// the endpoint does not exist.
pub fn methods_for(path: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for e in CATALOG.iter().filter(|e| path_matches(e.path, path)) {
        if !out.contains(&e.method) {
            out.push(e.method);
        }
    }
    out
}

/// Endpoints whose response carries text written by someone other than the user: feed
/// articles pulled from the open web, mail sent in by third parties, and the search index
/// that mixes both into its hits.
///
/// The gateway hands a tool result straight to a model, which has no way to tell a
/// newsletter's prose from its own operator's instructions. So these bodies are fenced and
/// labelled on the way out (see `mcp::fence_untrusted`) instead of arriving as plain data.
/// Matched as a path prefix, so `/api/mailbox/messages/{id}` inherits the mark from
/// `/api/mailbox/messages`.
///
/// Add a path here whenever an endpoint starts returning text the user did not type.
pub const UNTRUSTED_PATHS: &[&str] = &[
    "/api/feeds",
    "/api/feed-items",
    "/api/feed-dashboards",
    "/api/mailbox/messages",
    "/api/mailbox/senders",
    "/api/mailbox/links",
    "/api/search",
];

/// Whether a concrete path serves content from outside — see [`UNTRUSTED_PATHS`].
pub fn is_untrusted(path: &str) -> bool {
    UNTRUSTED_PATHS
        .iter()
        .any(|p| path == *p || path.strip_prefix(*p).is_some_and(|rest| rest.starts_with('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtest_strategy_and_indicator_writes_are_reachable() {
        let id = "11111111-1111-1111-1111-111111111111";
        for (method, path) in [
            ("POST", "/api/backtest/strategies".to_string()),
            ("PUT", format!("/api/backtest/strategies/{id}")),
            ("DELETE", format!("/api/backtest/strategies/{id}")),
            ("POST", "/api/backtest/indicators".to_string()),
            ("PUT", format!("/api/backtest/indicators/{id}")),
            ("DELETE", format!("/api/backtest/indicators/{id}")),
        ] {
            let e = lookup(method, &path)
                .unwrap_or_else(|| panic!("expected catalog entry for {method} {path}"));
            assert_eq!(e.module, "backtest");
        }
    }

    #[test]
    fn watchlist_level_writes_are_reachable() {
        let id = "22222222-2222-2222-2222-222222222222";
        for (method, path) in [
            ("GET", "/api/watchlists/templates".to_string()),
            ("PATCH", format!("/api/watchlists/{id}")),
            ("DELETE", format!("/api/watchlists/{id}")),
        ] {
            let e = lookup(method, &path)
                .unwrap_or_else(|| panic!("expected catalog entry for {method} {path}"));
            assert_eq!(e.module, "watchlists");
        }
    }

    /// The portfolio import crosses a module boundary: a watchlists-scoped token reaches a
    /// portfolio's asset list through it. It is exposed anyway (the module is unusable without
    /// its own import), so this pins the crossing as a decision rather than an accident: it is
    /// the one watchlists entry whose effect depends on data another key normally guards.
    #[test]
    fn watchlist_portfolio_import_is_a_deliberate_module_crossing() {
        let id = "22222222-2222-2222-2222-222222222222";
        let e = lookup("POST", &format!("/api/watchlists/{id}/import")).expect("exposed");
        assert_eq!(e.module, "watchlists");
    }

    /// The optimizer is only usable as a chain: ask which parameters exist, price the grid,
    /// then start it. `params` and `estimate` compute (they write nothing), `optimize` does not:
    /// it takes every core for minutes, which is exactly the kind of thing a user should be
    /// asked about before it happens.
    #[test]
    fn the_optimizer_chain_is_reachable_and_only_its_reads_are_compute() {
        for (path, compute) in [
            ("/api/backtest/optimize/params", true),
            ("/api/backtest/optimize/estimate", true),
            ("/api/backtest/optimize", false),
        ] {
            let e = lookup("POST", path).unwrap_or_else(|| panic!("missing {path}"));
            assert_eq!(e.compute, compute, "{path} compute flag");
            assert!(e.body.is_some(), "{path} must publish its body schema");
        }
        assert!(lookup("GET", "/api/backtest/optimize/11111111-1111-1111-1111-111111111111").is_some());
    }

    /// Every declared body schema must actually generate: an object schema with
    /// `properties` (catches a struct whose schema derivation panics or degenerates).
    #[test]
    fn every_body_schema_generates_an_object_schema() {
        for e in CATALOG {
            let Some(body) = e.body else { continue };
            let s = body();
            assert!(
                s.get("properties").is_some_and(|p| p.is_object()),
                "{} {} has a degenerate body schema: {s}",
                e.method,
                e.path,
            );
        }
    }

    /// Every backtest payload field is backed by a static Rust type, so none of them may be
    /// advertised as "any JSON" (`true`, or a bare description with no type/$ref/enum).
    /// `settings` and `definition` used to be exactly that, and an agent handed an untyped
    /// field invents plausible-looking names — the observed 400s were `mode: "percent_equity"`,
    /// `sizing.kind: "percent"`, an operand `kind: "value"` and a missing `nodes`. A schema
    /// that says nothing is worse than none here: the catalog instructs the model to follow it
    /// exactly.
    ///
    /// Scoped to backtest on purpose. Elsewhere a handful of fields are genuinely free-form
    /// (template-driven `fields`/`cells`, editor block `content`) and have no static shape to
    /// publish; `SaveBody`'s `settings`/`stats` are legacy echo blobs.
    #[test]
    fn backtest_body_properties_are_never_untyped() {
        for e in CATALOG.iter().filter(|e| e.module == "backtest") {
            let Some(body) = e.body else { continue };
            let s = body();
            let Some(props) = s.get("properties").and_then(|p| p.as_object()) else { continue };
            for (name, p) in props {
                if e.path == "/api/backtest/runs" && matches!(name.as_str(), "settings" | "stats") {
                    continue; // legacy echo fields on SaveBody, deliberately opaque
                }
                let described = p.is_object()
                    && ["type", "$ref", "enum", "const", "oneOf", "anyOf", "allOf", "items"]
                        .iter()
                        .any(|k| p.get(*k).is_some());
                assert!(
                    described,
                    "{} {}: property `{name}` is advertised as untyped ({p})",
                    e.method,
                    e.path,
                );
            }
        }
    }

    /// Every `$ref` a body schema emits must resolve inside that same schema's own
    /// `definitions`. Guards the switch away from inlined subschemas: a dangling ref would
    /// leave the agent with a payload shape it cannot see.
    #[test]
    fn every_ref_resolves_within_its_schema() {
        fn refs(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, c) in m {
                        if k == "$ref" {
                            if let Some(r) = c.as_str() {
                                out.push(r.to_string());
                            }
                        }
                        refs(c, out);
                    }
                }
                Value::Array(a) => a.iter().for_each(|c| refs(c, out)),
                _ => {}
            }
        }
        for e in CATALOG {
            let Some(body) = e.body else { continue };
            let s = body();
            let mut found = Vec::new();
            refs(&s, &mut found);
            for r in found {
                let name = r
                    .strip_prefix("#/definitions/")
                    .unwrap_or_else(|| panic!("{} {}: unexpected $ref form {r}", e.method, e.path));
                assert!(
                    s.pointer(&format!("/definitions/{name}")).is_some(),
                    "{} {}: $ref {r} does not resolve",
                    e.method,
                    e.path,
                );
            }
        }
    }

    /// A module page hoists every endpoint's `definitions` into one shared block, so two
    /// endpoints of the same module must never define the same name differently — the second
    /// would silently overwrite the first and misdescribe one of the payloads.
    #[test]
    fn hoisted_definitions_do_not_collide_within_a_module() {
        for (module, _) in MODULES {
            let mut seen: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            for e in CATALOG.iter().filter(|e| e.module == *module) {
                let Some(body) = e.body else { continue };
                let Some(Value::Object(defs)) = body().as_object_mut().and_then(|o| o.remove("definitions"))
                else {
                    continue;
                };
                for (k, v) in defs {
                    if let Some(prev) = seen.get(&k) {
                        assert_eq!(
                            *prev, v,
                            "module {module}: definition `{k}` differs between endpoints \
                             (hoisting would drop one)"
                        );
                    } else {
                        seen.insert(k, v);
                    }
                }
            }
        }
    }

    /// The backtest settings contract, pinned against the exact guesses that failed in
    /// production: `mode` is the side selector (not a sizing mode), `sizing` is discriminated
    /// by `mode` (not `kind`), and an operand is discriminated by `kind` (with no `value`
    /// variant). All four were 400s before the engine types carried a schema.
    /// Follow a schema node to the definition it points at. schemars wraps a `$ref` in
    /// `allOf` whenever the field also carries a doc comment, so both forms occur.
    fn deref<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
        let r = node
            .get("$ref")
            .or_else(|| node.get("allOf").and_then(|a| a.get(0)).and_then(|f| f.get("$ref")))
            .and_then(|r| r.as_str());
        match r {
            Some(r) => root
                .pointer(&r.replace("#/definitions/", "/definitions/"))
                .unwrap_or_else(|| panic!("$ref {r} does not resolve")),
            None => node,
        }
    }

    #[test]
    fn backtest_run_schema_pins_the_engine_discriminators() {
        let e = lookup("POST", "/api/backtest/run").expect("run endpoint");
        let s = (e.body.expect("has body schema"))();
        let settings = deref(&s, &s["properties"]["settings"]);
        assert!(settings.get("properties").is_some(), "settings must resolve to a real schema");

        let required: Vec<&str> =
            settings["required"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(required.contains(&"mode") && required.contains(&"sizing"));

        // `mode` is long|short|both — the run that sent "percent_equity" here got a 400.
        let modes: Vec<&str> = deref(&s, &settings["properties"]["mode"])["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(modes, ["long", "short", "both"]);

        // `sizing` is tagged by `mode`, and percent_equity carries `percent`.
        let sizing = deref(&s, &settings["properties"]["sizing"]);
        let tags: Vec<&str> = sizing["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["properties"]["mode"]["const"].as_str())
            .collect();
        assert_eq!(tags, ["percent_equity", "fixed_qty", "risk", "equity_tiers", "kelly"]);
        let pe = sizing["oneOf"][0]["required"].as_array().unwrap();
        assert!(pe.iter().any(|v| v == "percent"));

        // Operands are tagged by `kind`; there is no `value` variant (another observed 400).
        let operand = s
            .pointer("/definitions/Operand")
            .expect("Operand definition is hoisted with the rest");
        let kinds: Vec<&str> = operand["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["properties"]["kind"]["const"].as_str())
            .collect();
        assert_eq!(
            kinds,
            ["price", "const", "indicator", "custom_indicator", "metric", "position"]
        );
    }

    /// The custom-indicator DAG advertises `nodes` as required — a run once failed with
    /// `missing field nodes` because `definition` was published as untyped.
    #[test]
    fn indicator_schema_requires_the_node_list() {
        let e = lookup("POST", "/api/backtest/indicators").expect("indicator endpoint");
        let s = (e.body.expect("has body schema"))();
        let def = deref(&s, &s["properties"]["definition"]);
        assert!(def["required"].as_array().unwrap().iter().any(|v| v == "nodes"));
        assert!(def["properties"]["nodes"]["items"].get("$ref").is_some());
    }

    /// A GET never takes a body, and a DELETE only where the handler genuinely identifies its
    /// target by payload. The constructors make this structural; this test guards against
    /// someone bypassing them with a literal `Endpoint { .. }`, and keeps the payload-addressed
    /// deletes down to a list short enough to read.
    #[test]
    fn reads_and_deletes_have_no_body_schema() {
        const DELETE_BY_BODY: &[&str] = &["/api/mindset/entries"];
        for e in CATALOG {
            if e.method == "GET" {
                assert!(e.body.is_none(), "GET {} must not declare a body", e.path);
            }
            if e.method == "DELETE" && e.body.is_some() {
                assert!(
                    DELETE_BY_BODY.contains(&e.path),
                    "DELETE {} declares a body but is not a payload-addressed delete",
                    e.path
                );
            }
        }
    }

    /// A literal path declared *after* a `{param}` sibling must still resolve to itself.
    /// `/api/watchlists/{id}` precedes `/api/watchlists/search` in the catalog and matches it,
    /// so a plain first-match scan returned the `{id}` entry — harmless while the two agree on
    /// module and `compute`, silently wrong the day they do not.
    #[test]
    fn a_literal_path_wins_over_an_earlier_param_template() {
        for path in [
            "/api/watchlists/search",
            "/api/watchlists/templates",
            "/api/mportfolios/snapshots",
            "/api/subscriptions/breakdown",
            "/api/subscriptions/settings",
            "/api/community-docs/favorites",
        ] {
            let e = lookup("GET", path).unwrap_or_else(|| panic!("no entry for {path}"));
            assert_eq!(e.path, path, "GET {path} resolved to the wrong catalog entry");
        }
        assert_eq!(lookup("PATCH", "/api/subscriptions/settings").unwrap().path, "/api/subscriptions/settings");
        // Templates still match concrete ids.
        assert_eq!(
            lookup("GET", "/api/watchlists/11111111-1111-1111-1111-111111111111").unwrap().path,
            "/api/watchlists/{id}"
        );
    }

    /// A GET carries no body schema, so its REQUIRED query parameters exist only in the
    /// description — and an agent cannot guess one it was never told about. `/findb/facets`
    /// was described as "Facet values for filtering" with no mention of `column`, and the
    /// live run failed with `missing field \`column\``. Every handler below takes a `Query<T>`
    /// with a non-`Option`, no-`default` field; the description must name it.
    #[test]
    fn required_query_params_are_named_in_the_description() {
        for (path, params) in [
            ("/api/automator/catalog/schema", &["method", "path"][..]),
            ("/api/findb/facets", &["column"][..]),
            ("/api/findb/search", &["q", "kind"][..]),
            ("/api/portfolios/search", &["q", "kind"][..]),
            ("/api/watchlists/search", &["q", "kind"][..]),
        ] {
            let e = lookup("GET", path).unwrap_or_else(|| panic!("no catalog entry for {path}"));
            for p in params {
                assert!(
                    e.desc.contains(p),
                    "GET {path}: required query param `{p}` is not named in the description",
                );
            }
        }
    }

    /// The incident that motivated body schemas: agents guessed `symbol`/`interval`
    /// and omitted `asset_type` on the download endpoint. Pin its real contract.
    #[test]
    fn histdata_download_schema_names_the_real_fields() {
        let e = lookup("POST", "/api/histdata/downloads").expect("download endpoint");
        let s = (e.body.expect("has body schema"))();
        let props = s["properties"].as_object().expect("object schema");
        for field in ["asset_type", "ticker", "timeframe", "from", "to", "provider"] {
            assert!(props.contains_key(field), "missing property {field}: {s}");
        }
        let required: Vec<&str> = s["required"]
            .as_array()
            .expect("required list")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for field in ["asset_type", "ticker", "timeframe", "from", "to"] {
            assert!(required.contains(&field), "{field} should be required");
        }
    }

    /// The Automator is exposed for authoring and nothing else. Composing a graph is the
    /// hardest thing the module asks of anyone, so an agent is welcome to it; running one
    /// is a different grant, because a workflow executes under its own `mcp_tokens`
    /// envelope and a caller able to both write a graph and start it would inherit
    /// whatever that envelope holds, whatever its own token says.
    ///
    /// This pins the line. Anything added to the reachable half must keep the property
    /// that a graph written through this gateway cannot run until a human saves it.
    #[test]
    fn the_automator_is_reachable_for_authoring_only() {
        for (method, path) in [
            ("GET", "/api/automator/workflows"),
            ("GET", "/api/automator/workflows/11111111-1111-1111-1111-111111111111"),
            ("GET", "/api/automator/workflows/11111111-1111-1111-1111-111111111111/versions"),
            ("GET", "/api/automator/catalog"),
            ("GET", "/api/automator/catalog/schema"),
            ("GET", "/api/automator/runs"),
            ("GET", "/api/automator/runs/11111111-1111-1111-1111-111111111111"),
            ("POST", "/api/automator/workflows"),
            ("PUT", "/api/automator/workflows/11111111-1111-1111-1111-111111111111"),
            ("PATCH", "/api/automator/workflows/11111111-1111-1111-1111-111111111111"),
            ("POST", "/api/automator/workflows/11111111-1111-1111-1111-111111111111/test"),
        ] {
            let e = lookup(method, path)
                .unwrap_or_else(|| panic!("{method} {path} should be reachable"));
            assert_eq!(e.module, "automator", "{method} {path} is scoped to the wrong module");
        }

        // The other half. Each of these either decides what runs live, runs it, or hands
        // out the envelope, so each is the user's own call.
        for (method, path) in [
            // Starts a real run, spending the whole envelope.
            ("POST", "/api/automator/workflows/11111111-1111-1111-1111-111111111111/run"),
            // Writes the live graph, so it is a save by another name.
            ("POST", "/api/automator/workflows/11111111-1111-1111-1111-111111111111/rollback"),
            // Destroys a user's automation.
            ("DELETE", "/api/automator/workflows/11111111-1111-1111-1111-111111111111"),
            // Runs an arbitrary block under an arbitrary workflow's envelope: the whole
            // escalation in one call.
            ("POST", "/api/automator/nodes/test"),
            // A schedule is what makes a graph run unattended.
            ("GET", "/api/automator/schedules"),
            ("POST", "/api/automator/schedules"),
            ("PATCH", "/api/automator/schedules/11111111-1111-1111-1111-111111111111"),
            ("DELETE", "/api/automator/schedules/11111111-1111-1111-1111-111111111111"),
            ("GET", "/api/automator/agenda"),
            ("POST", "/api/automator/runs/11111111-1111-1111-1111-111111111111/cancel"),
        ] {
            assert!(
                lookup(method, path).is_none(),
                "{method} {path} must stay off the gateway",
            );
        }
    }

    /// A test run is `compute`, so it does not ask the user to approve it. That is only
    /// honest because `automator_api::test_run` seals an automated one: notifications and
    /// external calls forced off, refused outright on a workflow carrying a token. The
    /// save is not compute, and neither is the create: they change what the user sees.
    #[test]
    fn only_the_automator_test_run_is_compute() {
        let test = lookup("POST", "/api/automator/workflows/1/test").expect("test entry");
        assert!(test.compute, "a sealed test run should not prompt");
        for (method, path) in
            [("PUT", "/api/automator/workflows/1"), ("POST", "/api/automator/workflows")]
        {
            let e = lookup(method, path).expect("entry");
            assert!(!e.compute, "{method} {path} writes something the user would miss");
        }
    }

    /// Every prefix in the untrusted list must name a real allowlisted path, or the mark is
    /// documentation nobody applies. The reverse guard is the point: mail and feed bodies
    /// reach the model verbatim, so they may never be served unfenced.
    #[test]
    fn untrusted_prefixes_cover_the_endpoints_that_serve_outside_text() {
        for prefix in UNTRUSTED_PATHS {
            assert!(
                CATALOG.iter().any(|e| e.path.starts_with(prefix)),
                "{prefix} marks no catalog entry"
            );
        }
        for path in [
            "/api/feed-items",
            "/api/feed-dashboards/11111111-1111-1111-1111-111111111111",
            "/api/mailbox/messages",
            "/api/mailbox/messages/11111111-1111-1111-1111-111111111111",
            "/api/mailbox/senders",
            "/api/search",
        ] {
            assert!(is_untrusted(path), "{path} must be fenced");
        }
        // The user's own writing is not fenced: a warning printed on everything is read on
        // nothing, and the journal holds no third-party text.
        for path in ["/api/journal/trades", "/api/backtest/run", "/api/feedback"] {
            assert!(!is_untrusted(path), "{path} must not be fenced");
        }
        // A prefix stops at a segment boundary.
        assert!(!is_untrusted("/api/searchable"));
    }
}
