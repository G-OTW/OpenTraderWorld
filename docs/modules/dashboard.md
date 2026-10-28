# Dashboard & navigation

The home screen of the app, plus the two things that sit above every module: the search box and the notification inbox.

## Dashboard pages

The dashboard ships five preset pages, one per way of working: **Investor** (long-horizon holdings), **Trader** (execution and market session), **Quant** (research, backtesting and algo work), **Portfolio** (holdings) and **Utilities** (automation, notes and tools). Each starts with a ready-made set of widgets. They are seeded once and are ordinary pages after that: rearrange them as you like. Only your own pages can be deleted.

You also create **your own pages** with **Custom +** in the sidebar's **Workspace** section, which lists your dashboards. Each has a name, an optional description, and a short **tag** shown in the favorites bar. The **favorites bar** at the top holds one to five pages, added or removed with the star next to it; its first entry is the page the dashboard opens on. When the bar is full, a new star asks which favorite to replace.

Use them the way a trading day splits: a *Morning* page with the news feed, the economic calendar and the routine checklist; a *Positions* page with the portfolio and the watchlist; an *Admin* page with todos and timers.

## Editing a layout

**Customize** opens the **Edit layout** mode. A page is a free grid of 12 columns: tiles flow left to right and wrap onto the next line, and no rows are stored. In edit mode you can:

- add **module tiles** (a link to a module, and the same module may appear on any number of pages) or **widgets** with the **Add** tile; **Add widget** in the header also works outside edit mode;
- drag a tile anywhere: the tiles after it move along and the grid reflows; **resize** any tile by column span;
- set the height of a line (**Compact**, **Standard** or **Tall**) and open a widget's **config** (the gear on the tile), where the **Title** height turns a widget into a full-width header band, for a Free text section title;
- insert a **Spacer**: a full-width gap of adjustable height.

Tiles are links, not copies: removing one from a page never touches the module or its data.

## Widgets

A widget is a live, interactive preview of a module: it reads and writes through that module's own API, so what you do in the widget is real. Widgets whose module isn't installed simply aren't offered.

Most widgets come in several **variants**, each a different view of the same data: Net worth, for example, offers the current value, its change, a trend, assets vs liabilities, a split by category, stale assets and top assets. **Add widget** lists every variant as its own card, and **Widget type** in the widget's config switches it later. The table lists what each widget covers across its variants.

| Widget | What it does |
|---|---|
| **Free text** | A note or heading you write yourself, markdown-lite. |
| **Notes** | Recently edited Editor docs, how many you have, and a one-line note creator. |
| **News feed** | Latest items from a chosen feed, as a list or grid. |
| **Mailbox** | The latest unread mail, newest first. |
| **Time tracker** | Start/stop a project timer without leaving the page. |
| **Quick trade** | Pick a category + template and open the add-trade form. |
| **Journal analytics** | Net PnL, win rate, profit factor, equity curve, open positions, exposure, behaviour insights and a daily PnL heatmap. |
| **Goals** | A short list of goals with progress; add one inline. |
| **ToDo** | Open tasks, tickable in place. |
| **Trading routine** | Today's checklist, tickable in place. |
| **Mindset** | The day's check-in. |
| **Reminder** | A quick add-reminder form. |
| **Reminder stats** | Unread notifications, upcoming and overdue reminders, and the most triggered one. |
| **Calendar** | Today & this week at a glance. |
| **Economic calendar** | Upcoming macro events, compressed. |
| **Portfolio** | A portfolio summary with live value. |
| **Subscriptions** | Monthly recurring spend, then what renews next. |
| **Net worth** | Current net worth, its change over a window you set, and a sparkline. |
| **Watchlist** | Live quotes for a chosen list: price, 24h and 7d change. |
| **Fundamentals** | Macro series and boards, a company snapshot, a statement line by quarter, year or TTM, sortable companies, filtered filings, upcoming earnings and valuation against stored peers. Read from stored data without spending provider quota. |
| **Automator** | Running workflows, next scheduled runs, success rate by workflow, recent failures and favorite quick runs. |
| **Webhooks** | Events received, recent events, error rate per endpoint, and endpoints that went silent. |
| **Historical data** | Stored datasets, storage size, the download queue, coverage gaps, freshness and failed downloads. |
| **Backtest** | Best saved run, the leaderboard, the strategy library and quick-run favorites. |
| **Finance database** | Favorites by classification and folder, the catalog update and stale favorites. |
| **Charts** | Saved layouts and lists, active chart alerts and recent alert activity. |
| **Managers' portfolios** | Tracked managers, the largest books, holdings drift and top holdings by change. |
| **Community Docs** | Saved references by category, recent additions and quick access. |
| **Quant tools** | Available datasets and backtests, single-asset risk and drawdown, correlation, seasonality, realized volatility against its historical range, and the estimated market regime. |
| **Tax calculator** | Estimated tax for the year, the effective rate and a year-over-year table. |
| **Prompt store** | Your prompts by tag, click one to copy it. |
| **Resources** | Bookmarks from a chosen category. |
| **Agent** | Ask the assistant: pick model and tools, send, land in the conversation. |
| **Agent activity** | Recent conversations and token usage. |

Fundamentals and Quant widgets refresh every five minutes while the page is visible. They keep their previous result during a refresh and explain a failed update. Fundamentals reads stored snapshots only; load or update missing data on the corresponding module page.

In **widget settings**, choose a Quant dataset or a basket of two to twenty compatible datasets. Basket members must share a timeframe; intraday members must also share a provider. Risk offers 90%, 95% or 99% historical VaR confidence, seasonality offers returns, volatility, volume or bar range, and volatility and regime cards expose their window or state count. Each analysis displays its actual history and sample size.

Fundamentals settings let you choose and order a macro board's series, pick up to four company metrics or table columns, select peer companies, and filter filings and earnings to followed companies. Statement TTM sums four consecutive quarters and is available for income and cash-flow lines; balance-sheet values remain period-end observations. A year-on-year comparison requires the same fiscal period in the previous year and a positive comparison base.

Hover, focus or tap a heatmap cell to inspect the value and its sample count. Missing cells are hatched, distinct from a measured zero. Narrow cards show monthly seasonality summaries or the strongest pair correlations. Widget links open the relevant module tab with the selected series, dataset or basket.

On screens up to 767px wide, dashboard cards stack in a single column, and in edit mode **Move up** / **Move down** buttons replace dragging. The saved arrangement remains available on wider screens.

## Global search

The search box in the top bar, focused from anywhere with <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, or plain <kbd>/</kbd> when you aren't typing in a field.

By default it matches **module names**, **Settings sections** and **Resources** entries. The **layers toggle** next to the box widens it to your content: Editor pages, Goals, Calendar events, ToDos, Routines, Reminders, Prompts, mail subjects, Community Docs, and Fundamentals companies, series and documents.

Two things it deliberately does not do: it matches **titles and names only, never bodies**, and it only searches modules you have installed. Results come back grouped by type, prefix matches first; <kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd> navigate them.

## Notifications

The bell in the top bar carries an unread count and opens the **notification inbox**, where [RemindMe](/modules/productivity#remindme) reminders land when they fire, along with notifications from other modules: Automator workflows, inbound [webhooks](/modules/productivity#webhooks), background jobs such as mailbox polls, data downloads and broker syncs, and security events such as a sign-in from an address this account has never used. A notification that fires while you're in the app also slides in as a banner.

Delivery to **email, Telegram, Slack or Discord** goes through the shared [notification channels](/config/settings#notifications) in Settings, where you also decide which modules may push to each one. The inbox itself is always on and needs no setup.
