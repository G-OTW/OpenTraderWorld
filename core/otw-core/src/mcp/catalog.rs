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

pub struct Endpoint {
    pub module: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub desc: &'static str,
}

/// Human labels for the permission UI, in display order.
pub const MODULES: &[(&str, &str)] = &[
    ("journal", "Trading Journal"),
    ("portfolios", "Portfolio Tracker"),
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
    Endpoint { module: "journal", method: "GET", path: "/api/journal/trades", desc: "List trades. Query: status, category_id, strategy_id, symbol, from, to, limit, offset." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/trades", desc: "Create a trade (template fields + typed cols: symbol, side, qty, prices, fees…)." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/trades/{id}", desc: "Get one trade with legs/brackets and computed PnL." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/trades/{id}", desc: "Update a trade." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/trades/{id}", desc: "Delete a trade." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/trade-suggestions", desc: "Autocomplete values seen in past trades (symbols…)." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/breakdown", desc: "Stats/PnL breakdown. Query: from, to, category_id, group (day|week|month|strategy|symbol…)." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/categories", desc: "List trade categories (accounts/books)." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/categories", desc: "Create a category." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/categories/{id}", desc: "Update a category." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/categories/{id}", desc: "Delete a category." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/categories/{id}/capital", desc: "List capital events (deposits/withdrawals) for a category." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/categories/{id}/capital", desc: "Add a capital event." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/capital/{id}", desc: "Delete a capital event." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/strategies", desc: "List strategies." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/strategies", desc: "Create a strategy." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/strategies/{id}", desc: "Update a strategy." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/strategies/{id}", desc: "Delete a strategy." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/templates", desc: "List trade-form templates." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/templates", desc: "Create a template." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/templates/{id}", desc: "Update a template." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/templates/{id}", desc: "Delete a template." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/fee-schedules", desc: "List fee schedules." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/fee-schedules", desc: "Create a fee schedule." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/fee-schedules/{id}", desc: "Update a fee schedule." },
    Endpoint { module: "journal", method: "DELETE", path: "/api/journal/fee-schedules/{id}", desc: "Delete a fee schedule." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/settings", desc: "Journal settings (display currency…)." },
    Endpoint { module: "journal", method: "PATCH", path: "/api/journal/settings", desc: "Update journal settings." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/fx/pending", desc: "FX conversions awaiting a manual rate." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/fx/quotes", desc: "Latest FX quotes (USD-based)." },
    Endpoint { module: "journal", method: "GET", path: "/api/journal/fx/rates/{date}", desc: "FX rates on a date (YYYY-MM-DD)." },
    Endpoint { module: "journal", method: "POST", path: "/api/journal/fx/rates/{date}", desc: "Resolve a pending date with manual rates." },
    // ── portfolios ───────────────────────────────────────────────────────────
    Endpoint { module: "portfolios", method: "GET", path: "/api/portfolios", desc: "List portfolios with valuation summary." },
    Endpoint { module: "portfolios", method: "POST", path: "/api/portfolios", desc: "Create a portfolio." },
    Endpoint { module: "portfolios", method: "GET", path: "/api/portfolios/search", desc: "Search priceable assets. Query: q." },
    Endpoint { module: "portfolios", method: "GET", path: "/api/portfolios/{id}", desc: "Portfolio detail: positions, valuation, history." },
    Endpoint { module: "portfolios", method: "PATCH", path: "/api/portfolios/{id}", desc: "Update portfolio settings." },
    Endpoint { module: "portfolios", method: "DELETE", path: "/api/portfolios/{id}", desc: "Delete a portfolio." },
    Endpoint { module: "portfolios", method: "POST", path: "/api/portfolios/{id}/refresh", desc: "Re-price the portfolio now." },
    Endpoint { module: "portfolios", method: "POST", path: "/api/portfolios/{id}/reconcile", desc: "Reconcile positions against a broker statement." },
    Endpoint { module: "portfolios", method: "POST", path: "/api/portfolios/{id}/assets", desc: "Add an asset/position." },
    Endpoint { module: "portfolios", method: "GET", path: "/api/portfolios/assets/{asset_id}", desc: "Asset detail with operations." },
    Endpoint { module: "portfolios", method: "PATCH", path: "/api/portfolios/assets/{asset_id}", desc: "Update an asset." },
    Endpoint { module: "portfolios", method: "DELETE", path: "/api/portfolios/assets/{asset_id}", desc: "Remove an asset." },
    Endpoint { module: "portfolios", method: "POST", path: "/api/portfolios/assets/{asset_id}/operations", desc: "Record a buy/sell/dividend operation." },
    Endpoint { module: "portfolios", method: "DELETE", path: "/api/portfolios/operations/{op_id}", desc: "Delete an operation." },
    // ── backtest ─────────────────────────────────────────────────────────────
    // Read/run surface only — strategy & indicator *writes* stay out of MCP by choice.
    Endpoint { module: "backtest", method: "POST", path: "/api/backtest/run", desc: "Run a backtest (strategy config + one or more datasets). Returns stats, trades, equity, benchmark, per-asset breakdown, and (grid) grid stats. Body: dataset_ids[], settings (engine Settings JSON)." },
    Endpoint { module: "backtest", method: "POST", path: "/api/backtest/align", desc: "Multi-asset alignment preview (no simulation): merged-clock length, overlap window, warm-up bars, per-asset inactive bars. Body: dataset_ids[], optional settings." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/runs", desc: "List saved runs (newest first)." },
    Endpoint { module: "backtest", method: "POST", path: "/api/backtest/runs", desc: "Save a run (settings + stats snapshot, optional strategy_id provenance)." },
    Endpoint { module: "backtest", method: "DELETE", path: "/api/backtest/runs/{id}", desc: "Delete a saved run." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/runs/{id}/report.md", desc: "Markdown report for a saved run (front matter, stats table, human-readable settings, exit reasons)." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/strategies", desc: "List saved strategies (named Settings)." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/strategies/{id}", desc: "Get one strategy with its full settings (feed straight to /run)." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/indicators", desc: "List saved custom indicators (node-graph definitions)." },
    Endpoint { module: "backtest", method: "GET", path: "/api/backtest/indicators/{id}", desc: "Get one custom-indicator definition." },
    // ── quant ────────────────────────────────────────────────────────────────
    Endpoint { module: "quant", method: "POST", path: "/api/quant/single", desc: "Risk/return metrics for one asset's series." },
    Endpoint { module: "quant", method: "POST", path: "/api/quant/kelly", desc: "Kelly criterion sizing." },
    Endpoint { module: "quant", method: "POST", path: "/api/quant/size", desc: "Position sizing calculator." },
    Endpoint { module: "quant", method: "POST", path: "/api/quant/asset-signals", desc: "Technical signals for an asset." },
    Endpoint { module: "quant", method: "POST", path: "/api/quant/portfolio", desc: "Portfolio-level metrics (correlations, vol, drawdown…)." },
    // ── histdata ─────────────────────────────────────────────────────────────
    Endpoint { module: "histdata", method: "GET", path: "/api/histdata/providers", desc: "Available market-data providers and their capabilities." },
    Endpoint { module: "histdata", method: "GET", path: "/api/histdata/datasets", desc: "List downloaded datasets." },
    Endpoint { module: "histdata", method: "GET", path: "/api/histdata/datasets/{id}/bars", desc: "OHLCV bars. Query: from, to, limit, offset." },
    Endpoint { module: "histdata", method: "POST", path: "/api/histdata/downloads", desc: "Queue a new dataset download (provider, symbol, interval, range)." },
    Endpoint { module: "histdata", method: "POST", path: "/api/histdata/datasets/{id}/append", desc: "Extend a dataset with newer bars." },
    Endpoint { module: "histdata", method: "GET", path: "/api/histdata/jobs", desc: "Download queue/job status." },
    Endpoint { module: "histdata", method: "DELETE", path: "/api/histdata/datasets/{id}", desc: "Delete a dataset." },
    // ── findb ────────────────────────────────────────────────────────────────
    Endpoint { module: "findb", method: "GET", path: "/api/findb/status", desc: "FinanceDatabase install status." },
    Endpoint { module: "findb", method: "GET", path: "/api/findb/search", desc: "Search financial products. Query: q, kind, exchange, country, sector, limit, offset." },
    Endpoint { module: "findb", method: "GET", path: "/api/findb/facets", desc: "Facet values for filtering (kinds, exchanges, countries…)." },
    Endpoint { module: "findb", method: "GET", path: "/api/findb/folders", desc: "List favorite folders." },
    Endpoint { module: "findb", method: "POST", path: "/api/findb/folders", desc: "Create a folder." },
    Endpoint { module: "findb", method: "PATCH", path: "/api/findb/folders/{id}", desc: "Rename a folder." },
    Endpoint { module: "findb", method: "DELETE", path: "/api/findb/folders/{id}", desc: "Delete a folder." },
    Endpoint { module: "findb", method: "GET", path: "/api/findb/favorites", desc: "List favorite products." },
    Endpoint { module: "findb", method: "POST", path: "/api/findb/favorites", desc: "Add a favorite." },
    Endpoint { module: "findb", method: "PATCH", path: "/api/findb/favorites/{id}", desc: "Move/annotate a favorite." },
    Endpoint { module: "findb", method: "DELETE", path: "/api/findb/favorites/{id}", desc: "Remove a favorite." },
    // ── mportfolios ──────────────────────────────────────────────────────────
    Endpoint { module: "mportfolios", method: "GET", path: "/api/mportfolios", desc: "List famous managers' portfolios (Dataroma cache)." },
    Endpoint { module: "mportfolios", method: "GET", path: "/api/mportfolios/{slug}", desc: "One manager's holdings." },
    Endpoint { module: "mportfolios", method: "POST", path: "/api/mportfolios/refresh", desc: "Refresh the cache from source." },
    Endpoint { module: "mportfolios", method: "GET", path: "/api/mportfolios/snapshots", desc: "List saved snapshots." },
    Endpoint { module: "mportfolios", method: "POST", path: "/api/mportfolios/snapshots", desc: "Snapshot current holdings for comparison." },
    Endpoint { module: "mportfolios", method: "GET", path: "/api/mportfolios/snapshots/{id}", desc: "Snapshot detail." },
    Endpoint { module: "mportfolios", method: "DELETE", path: "/api/mportfolios/snapshots/{id}", desc: "Delete a snapshot." },
    // ── wealth ───────────────────────────────────────────────────────────────
    Endpoint { module: "wealth", method: "GET", path: "/api/wealth/assets", desc: "List wealth assets (accounts, property, holdings…)." },
    Endpoint { module: "wealth", method: "POST", path: "/api/wealth/assets", desc: "Add an asset." },
    Endpoint { module: "wealth", method: "PATCH", path: "/api/wealth/assets/{id}", desc: "Update an asset." },
    Endpoint { module: "wealth", method: "DELETE", path: "/api/wealth/assets/{id}", desc: "Delete an asset." },
    Endpoint { module: "wealth", method: "GET", path: "/api/wealth/assets/{id}/revisions", desc: "Value history of an asset." },
    Endpoint { module: "wealth", method: "POST", path: "/api/wealth/assets/{id}/revisions", desc: "Record a new valuation." },
    Endpoint { module: "wealth", method: "PATCH", path: "/api/wealth/revisions/{id}", desc: "Update a valuation." },
    Endpoint { module: "wealth", method: "DELETE", path: "/api/wealth/revisions/{id}", desc: "Delete a valuation." },
    Endpoint { module: "wealth", method: "GET", path: "/api/wealth/breakdown", desc: "Net-worth breakdown over time." },
    Endpoint { module: "wealth", method: "GET", path: "/api/wealth/templates", desc: "List asset templates." },
    Endpoint { module: "wealth", method: "POST", path: "/api/wealth/templates", desc: "Create an asset template." },
    Endpoint { module: "wealth", method: "PATCH", path: "/api/wealth/templates/{id}", desc: "Update a template." },
    Endpoint { module: "wealth", method: "DELETE", path: "/api/wealth/templates/{id}", desc: "Delete a template." },
    Endpoint { module: "wealth", method: "GET", path: "/api/wealth/settings", desc: "Wealth settings." },
    Endpoint { module: "wealth", method: "PATCH", path: "/api/wealth/settings", desc: "Update wealth settings." },
    // ── subscriptions ────────────────────────────────────────────────────────
    Endpoint { module: "subscriptions", method: "GET", path: "/api/subscriptions", desc: "List recurring subscriptions." },
    Endpoint { module: "subscriptions", method: "POST", path: "/api/subscriptions", desc: "Add a subscription." },
    Endpoint { module: "subscriptions", method: "GET", path: "/api/subscriptions/{id}", desc: "Subscription detail." },
    Endpoint { module: "subscriptions", method: "PATCH", path: "/api/subscriptions/{id}", desc: "Update a subscription." },
    Endpoint { module: "subscriptions", method: "DELETE", path: "/api/subscriptions/{id}", desc: "Delete a subscription." },
    Endpoint { module: "subscriptions", method: "GET", path: "/api/subscriptions/breakdown", desc: "Cost breakdown (monthly/yearly, by category)." },
    Endpoint { module: "subscriptions", method: "GET", path: "/api/subscriptions/settings", desc: "Subscription settings." },
    Endpoint { module: "subscriptions", method: "PATCH", path: "/api/subscriptions/settings", desc: "Update settings." },
    // ── taxcalc ──────────────────────────────────────────────────────────────
    Endpoint { module: "taxcalc", method: "GET", path: "/api/taxcalc/templates", desc: "Available country tax templates." },
    Endpoint { module: "taxcalc", method: "GET", path: "/api/taxcalc/profiles", desc: "List tax profiles." },
    Endpoint { module: "taxcalc", method: "POST", path: "/api/taxcalc/profiles", desc: "Create a profile." },
    Endpoint { module: "taxcalc", method: "GET", path: "/api/taxcalc/profiles/{id}", desc: "Profile detail." },
    Endpoint { module: "taxcalc", method: "PUT", path: "/api/taxcalc/profiles/{id}", desc: "Update a profile." },
    Endpoint { module: "taxcalc", method: "DELETE", path: "/api/taxcalc/profiles/{id}", desc: "Delete a profile." },
    Endpoint { module: "taxcalc", method: "POST", path: "/api/taxcalc/compute", desc: "Stateless tax computation from inputs." },
    Endpoint { module: "taxcalc", method: "GET", path: "/api/taxcalc/scenarios", desc: "List saved scenarios." },
    Endpoint { module: "taxcalc", method: "POST", path: "/api/taxcalc/scenarios", desc: "Create a scenario." },
    Endpoint { module: "taxcalc", method: "GET", path: "/api/taxcalc/scenarios/{id}", desc: "Scenario detail." },
    Endpoint { module: "taxcalc", method: "PUT", path: "/api/taxcalc/scenarios/{id}", desc: "Update a scenario." },
    Endpoint { module: "taxcalc", method: "DELETE", path: "/api/taxcalc/scenarios/{id}", desc: "Delete a scenario." },
    Endpoint { module: "taxcalc", method: "POST", path: "/api/taxcalc/scenarios/{id}/compute", desc: "Compute a saved scenario." },
    // ── editor ───────────────────────────────────────────────────────────────
    Endpoint { module: "editor", method: "GET", path: "/api/documents", desc: "List documents (tree)." },
    Endpoint { module: "editor", method: "POST", path: "/api/documents", desc: "Create a document." },
    Endpoint { module: "editor", method: "GET", path: "/api/documents/{id}", desc: "Document content (rich-text blocks JSON)." },
    Endpoint { module: "editor", method: "PATCH", path: "/api/documents/{id}", desc: "Update title/content." },
    Endpoint { module: "editor", method: "DELETE", path: "/api/documents/{id}", desc: "Delete a document." },
    Endpoint { module: "editor", method: "POST", path: "/api/documents/{id}/move", desc: "Move in the tree." },
    Endpoint { module: "editor", method: "GET", path: "/api/databases/{id}", desc: "Load an editor database (columns + rows)." },
    Endpoint { module: "editor", method: "POST", path: "/api/databases/{id}/columns", desc: "Add a column." },
    Endpoint { module: "editor", method: "PATCH", path: "/api/databases/columns/{col_id}", desc: "Update a column." },
    Endpoint { module: "editor", method: "DELETE", path: "/api/databases/columns/{col_id}", desc: "Delete a column." },
    Endpoint { module: "editor", method: "POST", path: "/api/databases/{id}/rows", desc: "Add a row." },
    Endpoint { module: "editor", method: "PATCH", path: "/api/databases/rows/{row_id}", desc: "Update a row." },
    Endpoint { module: "editor", method: "DELETE", path: "/api/databases/rows/{row_id}", desc: "Delete a row." },
    Endpoint { module: "editor", method: "POST", path: "/api/databases/rows/{row_id}/move", desc: "Reorder a row." },
    // ── todos ────────────────────────────────────────────────────────────────
    Endpoint { module: "todos", method: "GET", path: "/api/todos", desc: "List todos." },
    Endpoint { module: "todos", method: "POST", path: "/api/todos", desc: "Add a todo." },
    Endpoint { module: "todos", method: "GET", path: "/api/todos/{id}", desc: "Todo detail." },
    Endpoint { module: "todos", method: "PATCH", path: "/api/todos/{id}", desc: "Update a todo." },
    Endpoint { module: "todos", method: "DELETE", path: "/api/todos/{id}", desc: "Delete a todo." },
    Endpoint { module: "todos", method: "PATCH", path: "/api/todos/{id}/done", desc: "Toggle done." },
    // ── goals ────────────────────────────────────────────────────────────────
    Endpoint { module: "goals", method: "GET", path: "/api/goals", desc: "List goals with progress." },
    Endpoint { module: "goals", method: "POST", path: "/api/goals", desc: "Create a goal." },
    Endpoint { module: "goals", method: "GET", path: "/api/goals/{id}", desc: "Goal detail." },
    Endpoint { module: "goals", method: "PATCH", path: "/api/goals/{id}", desc: "Update a goal / progress." },
    Endpoint { module: "goals", method: "DELETE", path: "/api/goals/{id}", desc: "Delete a goal." },
    // ── remindme ─────────────────────────────────────────────────────────────
    Endpoint { module: "remindme", method: "GET", path: "/api/reminders", desc: "List reminders." },
    Endpoint { module: "remindme", method: "POST", path: "/api/reminders", desc: "Create a reminder (one-shot or recurring)." },
    Endpoint { module: "remindme", method: "GET", path: "/api/reminders/{id}", desc: "Reminder detail." },
    Endpoint { module: "remindme", method: "PATCH", path: "/api/reminders/{id}", desc: "Update a reminder." },
    Endpoint { module: "remindme", method: "DELETE", path: "/api/reminders/{id}", desc: "Delete a reminder." },
    Endpoint { module: "remindme", method: "GET", path: "/api/notifications", desc: "In-app notifications." },
    Endpoint { module: "remindme", method: "GET", path: "/api/notifications/unread", desc: "Unread count." },
    Endpoint { module: "remindme", method: "POST", path: "/api/notifications/ack-all", desc: "Mark all read." },
    Endpoint { module: "remindme", method: "POST", path: "/api/notifications/{id}/read", desc: "Mark one read." },
    // ── calendar ─────────────────────────────────────────────────────────────
    Endpoint { module: "calendar", method: "GET", path: "/api/calendar/events", desc: "List events. Query: from, to." },
    Endpoint { module: "calendar", method: "POST", path: "/api/calendar/events", desc: "Create an event." },
    Endpoint { module: "calendar", method: "GET", path: "/api/calendar/events/{id}", desc: "Event detail." },
    Endpoint { module: "calendar", method: "PATCH", path: "/api/calendar/events/{id}", desc: "Update an event." },
    Endpoint { module: "calendar", method: "DELETE", path: "/api/calendar/events/{id}", desc: "Delete an event." },
    // ── time ─────────────────────────────────────────────────────────────────
    Endpoint { module: "time", method: "GET", path: "/api/time/projects", desc: "List time-tracking projects." },
    Endpoint { module: "time", method: "POST", path: "/api/time/projects", desc: "Create a project." },
    Endpoint { module: "time", method: "GET", path: "/api/time/projects/{id}", desc: "Project detail." },
    Endpoint { module: "time", method: "PATCH", path: "/api/time/projects/{id}", desc: "Update a project." },
    Endpoint { module: "time", method: "DELETE", path: "/api/time/projects/{id}", desc: "Delete a project." },
    Endpoint { module: "time", method: "POST", path: "/api/time/projects/{id}/start", desc: "Start the timer on a project." },
    Endpoint { module: "time", method: "POST", path: "/api/time/projects/{id}/stop", desc: "Stop the timer." },
    Endpoint { module: "time", method: "GET", path: "/api/time/projects/{id}/entries", desc: "List entries for a project." },
    Endpoint { module: "time", method: "POST", path: "/api/time/projects/{id}/entries", desc: "Add a manual entry." },
    Endpoint { module: "time", method: "DELETE", path: "/api/time/entries/{id}", desc: "Delete an entry." },
    Endpoint { module: "time", method: "GET", path: "/api/time/state", desc: "Current running timer, if any." },
    Endpoint { module: "time", method: "GET", path: "/api/time/breakdown", desc: "Tracked-time breakdown." },
    // ── routines (trader board) ──────────────────────────────────────────────
    Endpoint { module: "routines", method: "GET", path: "/api/trader/board", desc: "Today's trader board: routines + tasks with check state." },
    Endpoint { module: "routines", method: "GET", path: "/api/trader/routines", desc: "List routines." },
    Endpoint { module: "routines", method: "POST", path: "/api/trader/routines", desc: "Create a routine." },
    Endpoint { module: "routines", method: "POST", path: "/api/trader/items/{id}/check", desc: "Check/uncheck a routine item today." },
    Endpoint { module: "routines", method: "POST", path: "/api/trader/tasks", desc: "Add a one-off task." },
    Endpoint { module: "routines", method: "PATCH", path: "/api/trader/tasks/{id}", desc: "Update a task." },
    Endpoint { module: "routines", method: "DELETE", path: "/api/trader/tasks/{id}", desc: "Delete a task." },
    // ── mindset ──────────────────────────────────────────────────────────────
    Endpoint { module: "mindset", method: "GET", path: "/api/mindset/day", desc: "Today's journal prompts + entries. Query: date." },
    Endpoint { module: "mindset", method: "PUT", path: "/api/mindset/entries", desc: "Save an entry for a prompt/date." },
    Endpoint { module: "mindset", method: "GET", path: "/api/mindset/history", desc: "Past entries." },
    Endpoint { module: "mindset", method: "GET", path: "/api/mindset/prompts", desc: "List prompts." },
    Endpoint { module: "mindset", method: "POST", path: "/api/mindset/prompts", desc: "Add a prompt." },
    Endpoint { module: "mindset", method: "PATCH", path: "/api/mindset/prompts/{id}", desc: "Update a prompt." },
    Endpoint { module: "mindset", method: "DELETE", path: "/api/mindset/prompts/{id}", desc: "Delete a prompt." },
    // ── news ─────────────────────────────────────────────────────────────────
    Endpoint { module: "news", method: "GET", path: "/api/feeds", desc: "List configured feeds." },
    Endpoint { module: "news", method: "POST", path: "/api/feeds", desc: "Add a feed (url, kind, poll interval)." },
    Endpoint { module: "news", method: "GET", path: "/api/feeds/{id}", desc: "Feed detail." },
    Endpoint { module: "news", method: "PATCH", path: "/api/feeds/{id}", desc: "Update a feed." },
    Endpoint { module: "news", method: "DELETE", path: "/api/feeds/{id}", desc: "Delete a feed." },
    Endpoint { module: "news", method: "POST", path: "/api/feeds/{id}/refresh", desc: "Poll one feed now." },
    Endpoint { module: "news", method: "POST", path: "/api/feeds/refresh-all", desc: "Poll all feeds now." },
    Endpoint { module: "news", method: "GET", path: "/api/feed-items", desc: "Aggregated items. Query: feed_id, search, limit, offset." },
    Endpoint { module: "news", method: "GET", path: "/api/feed-sources", desc: "Known source templates." },
    // ── resources ────────────────────────────────────────────────────────────
    Endpoint { module: "resources", method: "GET", path: "/api/resources", desc: "List saved resources (links/notes)." },
    Endpoint { module: "resources", method: "POST", path: "/api/resources", desc: "Add a resource." },
    Endpoint { module: "resources", method: "PATCH", path: "/api/resources/{id}", desc: "Update a resource." },
    Endpoint { module: "resources", method: "DELETE", path: "/api/resources/{id}", desc: "Delete a resource." },
    Endpoint { module: "resources", method: "GET", path: "/api/resources/categories", desc: "List resource categories." },
    Endpoint { module: "resources", method: "POST", path: "/api/resources/categories", desc: "Add a category." },
    Endpoint { module: "resources", method: "PATCH", path: "/api/resources/categories/{id}", desc: "Update a category." },
    Endpoint { module: "resources", method: "DELETE", path: "/api/resources/categories/{id}", desc: "Delete a category." },
    // ── prompt-store ─────────────────────────────────────────────────────────
    Endpoint { module: "prompt-store", method: "GET", path: "/api/prompts", desc: "List saved prompts (name, body, tags)." },
    Endpoint { module: "prompt-store", method: "POST", path: "/api/prompts", desc: "Create a prompt (name, body, tags)." },
    Endpoint { module: "prompt-store", method: "GET", path: "/api/prompts/{id}", desc: "Get one prompt." },
    Endpoint { module: "prompt-store", method: "PATCH", path: "/api/prompts/{id}", desc: "Edit a prompt (name, body, tags)." },
    // ── community-docs ───────────────────────────────────────────────────────
    Endpoint { module: "community-docs", method: "GET", path: "/api/community-docs", desc: "List community docs." },
    Endpoint { module: "community-docs", method: "GET", path: "/api/community-docs/{slug}", desc: "Read one doc." },
    Endpoint { module: "community-docs", method: "GET", path: "/api/community-docs/favorites", desc: "List favorites." },
    Endpoint { module: "community-docs", method: "PUT", path: "/api/community-docs/{slug}/favorite", desc: "Set/unset favorite." },
    Endpoint { module: "community-docs", method: "POST", path: "/api/community-docs/refresh", desc: "Refresh the catalog from the website." },
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
