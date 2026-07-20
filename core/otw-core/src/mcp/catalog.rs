//! MCP endpoint allowlist — the only REST routes reachable through the gateway tools.
//!
//! Security model: explicit allowlist, not passthrough. Anything absent here is
//! unreachable via MCP regardless of token permissions. Deliberately excluded:
//! account/session/settings/network admin, secret management (feed + provider API
//! keys, notification channels), data wipe, module install/detach, logs, binary
//! file upload/download, SSE streams, the FinanceDatabase bulk import, and the
//! community-docs external submission relay.
//!
//! `module` is the permission key tokens are scoped by (aligned with the frontend
//! module ids). GET needs `"r"`; other methods need `"rw"`.
//!
//! Request-body contract: every POST/PUT/PATCH entry carries a JSON Schema generated
//! (via schemars) from the exact struct its Axum handler deserializes, so what the
//! catalog advertises is what serde accepts — the schema cannot drift from the code.
//! The constructors enforce this at compile time: `post`/`put`/`patch` require a
//! schema, and a write endpoint that genuinely takes no body must say so with
//! `post_empty`. GET/DELETE never take a body.

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
}

/// JSON Schema for a handler's request-body struct. Subschemas are inlined so agents
/// get one self-contained object; `$schema`/`title` noise is stripped.
pub fn schema<T: schemars::JsonSchema>() -> Value {
    let mut settings = schemars::generate::SchemaSettings::draft07();
    settings.inline_subschemas = true;
    let root = settings.into_generator().into_root_schema_for::<T>();
    let mut v = serde_json::to_value(root).expect("schema serializes");
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
    }
    v
}

const fn get(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "GET", path, desc, body: None }
}

const fn delete(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "DELETE", path, desc, body: None }
}

const fn post(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: Some(body) }
}

/// A POST that deliberately takes no request body (action is fully named by the path).
const fn post_empty(module: &'static str, path: &'static str, desc: &'static str) -> Endpoint {
    Endpoint { module, method: "POST", path, desc, body: None }
}

const fn put(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "PUT", path, desc, body: Some(body) }
}

const fn patch(
    module: &'static str,
    path: &'static str,
    desc: &'static str,
    body: BodySchema,
) -> Endpoint {
    Endpoint { module, method: "PATCH", path, desc, body: Some(body) }
}

/// Human labels for the permission UI, in display order.
pub const MODULES: &[(&str, &str)] = &[
    ("journal", "Trading Journal"),
    ("portfolios", "Portfolio Tracker"),
    ("watchlists", "Watchlists"),
    ("backtest", "Backtest"),
    ("quant", "Quant Tools"),
    ("histdata", "Historical Data"),
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
    get("journal", "/api/journal/fx/pending", "FX conversions awaiting a manual rate."),
    get("journal", "/api/journal/fx/quotes", "Latest FX quotes (USD-based)."),
    get("journal", "/api/journal/fx/rates/{date}", "FX rates on a date (YYYY-MM-DD)."),
    post("journal", "/api/journal/fx/rates/{date}", "Resolve a pending date with manual rates.", schema::<crate::fx_api::ResolveBody>),
    // ── portfolios ───────────────────────────────────────────────────────────
    get("portfolios", "/api/portfolios", "List portfolios with valuation summary."),
    post("portfolios", "/api/portfolios", "Create a portfolio.", schema::<crate::portfolios_api::CreateBody>),
    get("portfolios", "/api/portfolios/search", "Search priceable assets. Query: q."),
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
    post("backtest", "/api/backtest/run", "Run a backtest (strategy config + one or more datasets). Pass view=\"summary\": the default \"full\" also returns every trade and the whole equity curve, which overflows the response budget on a long dataset. Summary returns stats, per-asset breakdown, alignment and run_id. Every run is recorded in the history automatically; name it via POST /api/backtest/runs with that run_id to keep it permanently.\n\nUNITS inside `settings` — the two families do NOT share a scale, and the `_pct` suffix does not tell them apart. Get a worked example from GET /api/backtest/strategies/{id} before composing one by hand.\nFRACTIONS (0.01 = 1%): long/short.stop_loss_pct, long/short.take_profit_pct, stop_loss.value + take_profit.value when kind=\"pct\", slippage.value when kind=\"pct\", spread_pct, pyramiding min_distance_pct, oos_split_pct. A realistic crypto slippage is 0.0005 (=0.05%); 0.05 here means 5% per fill and will wipe the account.\nPERCENTS (10 = 10%): sizing.percent, sizing.risk_pct, sizing cap_pct, risk.max_exposure_pct, risk.max_exposure_per_asset_pct, risk.max_daily_loss_pct, risk.max_drawdown_pct, funding.annual_rate_pct. Also fees.amount when amount_kind=\"pct\". sizing.fraction (Kelly) is a true fraction (0.5 = half Kelly); equity_tiers value follows its `metric`.\nEvery run echoes the settings it used back in the report — check the echoed slippage/spread/sizing against what you intended before trusting a result.", schema::<crate::backtest_api::RunBody>),
    post("backtest", "/api/backtest/align", "Multi-asset alignment preview (no simulation): merged-clock length, overlap window, warm-up bars, per-asset inactive bars.", schema::<crate::backtest_api::AlignBody>),
    get("backtest", "/api/backtest/runs", "Run history, newest first (every run lands here automatically). Query: filter=saved for the named/pinned runs only."),
    post("backtest", "/api/backtest/runs", "Name a run so it is kept permanently (pins it; never pruned by the history cap). Body: run_id (from /run) + name.", schema::<crate::backtest_api::SaveBody>),
    delete("backtest", "/api/backtest/runs", "Clear the auto history. Named/pinned runs are kept."),
    delete("backtest", "/api/backtest/runs/{id}", "Delete one run from the history."),
    get("backtest", "/api/backtest/runs/{id}/report.md", "Markdown report for a saved run (front matter, stats table, human-readable settings, exit reasons)."),
    get("backtest", "/api/backtest/strategies", "List saved strategies (named Settings)."),
    post("backtest", "/api/backtest/strategies", "Create a strategy (named engine Settings; validated). Returns id.", schema::<crate::backtest_api::StrategyBody>),
    get("backtest", "/api/backtest/strategies/{id}", "Get one strategy with its full settings (feed straight to /run)."),
    put("backtest", "/api/backtest/strategies/{id}", "Update a strategy.", schema::<crate::backtest_api::StrategyBody>),
    delete("backtest", "/api/backtest/strategies/{id}", "Delete a strategy."),
    get("backtest", "/api/backtest/indicators", "List saved custom indicators (node-graph definitions)."),
    post("backtest", "/api/backtest/indicators", "Create a custom indicator (node-graph DAG JSON; validated: bounded, no forward/self refs). Returns id.", schema::<crate::backtest_api::IndicatorBody>),
    get("backtest", "/api/backtest/indicators/{id}", "Get one custom-indicator definition."),
    put("backtest", "/api/backtest/indicators/{id}", "Update a custom indicator.", schema::<crate::backtest_api::IndicatorBody>),
    delete("backtest", "/api/backtest/indicators/{id}", "Delete a custom indicator."),
    // ── quant ────────────────────────────────────────────────────────────────
    post("quant", "/api/quant/single", "Risk/return metrics for one asset's series (VaR, CVaR, vol, drawdown). Response includes per-bar drawdown_curve + histogram for charting — pass pick (e.g. [\"result.var_hist\",\"result.cvar\"]) when the user only needs figures.", schema::<crate::quant_api::SingleBody>),
    post("quant", "/api/quant/kelly", "Kelly criterion sizing.", schema::<crate::quant_api::KellyBody>),
    post("quant", "/api/quant/size", "Position sizing calculator.", schema::<crate::quant_api::SizeBody>),
    post("quant", "/api/quant/asset-signals", "Technical signals for an asset.", schema::<crate::quant_api::AssetSignalsBody>),
    post("quant", "/api/quant/portfolio", "Portfolio-level metrics (correlations, vol, drawdown…). Large response — pick the metric fields the user asked for.", schema::<crate::quant_api::PortfolioBody>),
    // ── histdata ─────────────────────────────────────────────────────────────
    get("histdata", "/api/histdata/providers", "Available market-data providers and their capabilities (valid asset_type/timeframe combos per provider)."),
    get("histdata", "/api/histdata/datasets", "List downloaded datasets."),
    get("histdata", "/api/histdata/datasets/{id}/bars", "OHLCV bars. Query: from, to, limit, offset."),
    post("histdata", "/api/histdata/downloads", "Queue a new dataset download. Check /api/histdata/providers for valid provider/asset_type/timeframe combos first.", schema::<crate::histdata_api::DownloadBody>),
    post_empty("histdata", "/api/histdata/datasets/{id}/append", "Extend a dataset with newer bars (from its last bar to now)."),
    get("histdata", "/api/histdata/jobs", "Download queue/job status."),
    delete("histdata", "/api/histdata/datasets/{id}", "Delete a dataset."),
    // ── findb ────────────────────────────────────────────────────────────────
    get("findb", "/api/findb/status", "FinanceDatabase install status."),
    get("findb", "/api/findb/search", "Search financial products. Query: q, kind, exchange, country, sector, limit, offset."),
    get("findb", "/api/findb/facets", "Facet values for filtering (kinds, exchanges, countries…)."),
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
    patch("watchlists", "/api/watchlists/items/{id}", "Update an item (notes, position, quote overrides).", schema::<crate::watchlists_api::PatchItemBody>),
    delete("watchlists", "/api/watchlists/items/{id}", "Remove an item."),
    post_empty("watchlists", "/api/watchlists/{id}/refresh", "Re-quote every item now."),
    // ── mportfolios ──────────────────────────────────────────────────────────
    get("mportfolios", "/api/mportfolios", "List famous managers' portfolios (Dataroma cache)."),
    get("mportfolios", "/api/mportfolios/{slug}", "One manager's holdings."),
    post_empty("mportfolios", "/api/mportfolios/refresh", "Refresh the cache from source."),
    get("mportfolios", "/api/mportfolios/snapshots", "List saved snapshots."),
    post("mportfolios", "/api/mportfolios/snapshots", "Snapshot current holdings for comparison.", schema::<crate::mportfolios_api::SnapshotBody>),
    get("mportfolios", "/api/mportfolios/snapshots/{id}", "Snapshot detail."),
    delete("mportfolios", "/api/mportfolios/snapshots/{id}", "Delete a snapshot."),
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
    get("subscriptions", "/api/subscriptions/settings", "Subscription settings."),
    patch("subscriptions", "/api/subscriptions/settings", "Update settings.", schema::<crate::subscriptions_api::SettingsBody>),
    // ── taxcalc ──────────────────────────────────────────────────────────────
    get("taxcalc", "/api/taxcalc/templates", "Available country tax templates."),
    get("taxcalc", "/api/taxcalc/profiles", "List tax profiles."),
    post("taxcalc", "/api/taxcalc/profiles", "Create a profile.", schema::<crate::taxcalc_api::ProfileBody>),
    get("taxcalc", "/api/taxcalc/profiles/{id}", "Profile detail."),
    put("taxcalc", "/api/taxcalc/profiles/{id}", "Update a profile.", schema::<crate::taxcalc_api::ProfileBody>),
    delete("taxcalc", "/api/taxcalc/profiles/{id}", "Delete a profile."),
    post("taxcalc", "/api/taxcalc/compute", "Stateless tax computation from inputs.", schema::<crate::taxcalc_api::ScenarioBody>),
    get("taxcalc", "/api/taxcalc/scenarios", "List saved scenarios."),
    post("taxcalc", "/api/taxcalc/scenarios", "Create a scenario.", schema::<crate::taxcalc_api::ScenarioBody>),
    get("taxcalc", "/api/taxcalc/scenarios/{id}", "Scenario detail."),
    put("taxcalc", "/api/taxcalc/scenarios/{id}", "Update a scenario.", schema::<crate::taxcalc_api::ScenarioBody>),
    delete("taxcalc", "/api/taxcalc/scenarios/{id}", "Delete a scenario."),
    post_empty("taxcalc", "/api/taxcalc/scenarios/{id}/compute", "Compute a saved scenario."),
    // ── editor ───────────────────────────────────────────────────────────────
    get("editor", "/api/documents", "List documents (tree)."),
    post("editor", "/api/documents", "Create a document.", schema::<crate::documents::CreateBody>),
    get("editor", "/api/documents/{id}", "Document content (rich-text blocks JSON)."),
    patch("editor", "/api/documents/{id}", "Update title/content.", schema::<crate::documents::UpdateBody>),
    delete("editor", "/api/documents/{id}", "Delete a document."),
    post("editor", "/api/documents/{id}/move", "Move in the tree.", schema::<crate::documents::MoveBody>),
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
    get("time", "/api/time/state", "Current running timer, if any."),
    get("time", "/api/time/breakdown", "Tracked-time breakdown."),
    // ── routines (trader board) ──────────────────────────────────────────────
    get("routines", "/api/trader/board", "Today's trader board: routines + tasks with check state."),
    get("routines", "/api/trader/routines", "List routines."),
    post("routines", "/api/trader/routines", "Create a routine.", schema::<crate::trader_tasks_api::CreateRoutineBody>),
    post("routines", "/api/trader/items/{id}/check", "Check/uncheck a routine item today.", schema::<crate::trader_tasks_api::CheckBody>),
    post("routines", "/api/trader/tasks", "Add a one-off task.", schema::<crate::trader_tasks_api::AddTaskBody>),
    patch("routines", "/api/trader/tasks/{id}", "Update a task.", schema::<crate::trader_tasks_api::UpdateTaskBody>),
    delete("routines", "/api/trader/tasks/{id}", "Delete a task."),
    // ── mindset ──────────────────────────────────────────────────────────────
    get("mindset", "/api/mindset/day", "Today's journal prompts + entries. Query: date."),
    put("mindset", "/api/mindset/entries", "Save an entry for a prompt/date.", schema::<crate::mindset_api::SaveEntryBody>),
    get("mindset", "/api/mindset/history", "Past entries."),
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
    patch("prompt-store", "/api/prompts/{id}", "Edit a prompt.", schema::<otw_store::prompts::PromptInput>),
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
pub fn lookup(method: &str, path: &str) -> Option<&'static Endpoint> {
    CATALOG
        .iter()
        .find(|e| e.method == method && path_matches(e.path, path))
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

    /// The portfolio import crosses a module boundary (reads portfolios through a
    /// watchlists-scoped token), so it stays off the allowlist.
    #[test]
    fn watchlist_portfolio_import_is_not_exposed() {
        let id = "22222222-2222-2222-2222-222222222222";
        assert!(lookup("POST", &format!("/api/watchlists/{id}/import")).is_none());
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

    /// GET/DELETE never take a body; the constructors make this structural, this test
    /// guards against someone bypassing them with a literal `Endpoint { .. }`.
    #[test]
    fn reads_and_deletes_have_no_body_schema() {
        for e in CATALOG {
            if matches!(e.method, "GET" | "DELETE") {
                assert!(e.body.is_none(), "{} {} must not declare a body", e.method, e.path);
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
}
