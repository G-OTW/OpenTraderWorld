# Dashboard & navigation

The home screen of the app, plus the two things that sit above every module: the search box and the notification inbox.

## Dashboard pages

The dashboard opens on a built-in **Modules** page: a tile per installed module, rebuilt automatically as you install and detach. It is never edited or deleted; it just reflects what you have.

On top of that you create **your own pages**. Each has a name, an optional description, and a short **tag** shown on its chip. One page is the **default**: the one the dashboard opens on, and the one whose chip sorts first.

Use them the way a trading day splits: a *Morning* page with the news feed, the economic calendar and the routine checklist; a *Positions* page with the portfolio and the watchlist; an *Admin* page with todos and timers.

## Editing a layout

**Edit layout** turns a page into a grid of rows over 12 columns. In edit mode you can:

- **add rows** and drop **module tiles** (a link to a module, and the same module may appear on any number of pages) or **widgets** into them;
- **resize** any tile by column span, and drag tiles between rows;
- set a widget's **height preset** (compact, standard or tall) and open its **config** (the gear on the tile);
- insert **spacer rows** to breathe between blocks.

Tiles are links, not copies: removing one from a page never touches the module or its data.

## Widgets

A widget is a live, interactive preview of a module: it reads and writes through that module's own API, so what you do in the widget is real. Widgets whose module isn't installed simply aren't offered.

| Widget | What it does |
|---|---|
| **Free text** | A note or heading you write yourself, markdown-lite. |
| **News feed** | Latest items from a chosen feed, as a list or grid. |
| **Mailbox** | The latest unread mail, newest first. |
| **Time tracker** | Start/stop a project timer without leaving the page. |
| **Quick trade** | Pick a category + template and open the add-trade form. |
| **Goals** | A short list of goals with progress; add one inline. |
| **ToDo** | Open tasks, tickable in place. |
| **Trading routine** | Today's checklist, tickable in place. |
| **Mindset** | The day's check-in. |
| **Reminder** | A quick add-reminder form. |
| **Calendar** | Today & this week at a glance. |
| **Economic calendar** | Upcoming macro events, compressed. |
| **Portfolio** | A portfolio summary with live value. |
| **Subscriptions** | Monthly recurring spend, then what renews next. |
| **Net worth** | Current net worth, its change over a window you set, and a sparkline. |
| **Watchlist** | Live quotes for a chosen list: price, 24h and 7d change. |
| **Fundamentals** | Macro series and boards, a company snapshot, a statement line by quarter, year or TTM, sortable companies, filtered filings, upcoming earnings and valuation against stored peers. Read from stored data without spending provider quota. |
| **Quant** | Available datasets and backtests, single-asset risk and drawdown, correlation, seasonality, realized volatility against its historical range, and the estimated market regime. |
| **Prompt store** | Your prompts by tag, click one to copy it. |
| **Resources** | Bookmarks from a chosen category. |
| **Agent** | Ask the assistant: pick model and tools, send, land in the conversation. |

Fundamentals and Quant widgets refresh every five minutes while the page is visible. They keep their previous result during a refresh and explain a failed update. Fundamentals reads stored snapshots only; load or update missing data on the corresponding module page.

In **widget settings**, choose a Quant dataset or a basket of two to twenty compatible datasets. Basket members must share a timeframe; intraday members must also share a provider. Risk offers 90%, 95% or 99% historical VaR confidence, seasonality offers returns, volatility, volume or bar range, and volatility and regime cards expose their window or state count. Each analysis displays its actual history and sample size.

Fundamentals settings let you choose and order a macro board's series, pick up to four company metrics or table columns, select peer companies, and filter filings and earnings to followed companies. Statement TTM sums four consecutive quarters and is available for income and cash-flow lines; balance-sheet values remain period-end observations. A year-on-year comparison requires the same fiscal period in the previous year and a positive comparison base.

Hover, focus or tap a heatmap cell to inspect the value and its sample count. Missing cells are hatched, distinct from a measured zero. Narrow cards show monthly seasonality summaries or the strongest pair correlations. Widget links open the relevant module tab with the selected series, dataset or basket.

On phone screens up to 480px wide, dashboard cards stack in a single column. The saved arrangement remains available on wider screens and in the layout editor.

## Global search

The search box in the top bar, focused from anywhere with <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, or plain <kbd>/</kbd> when you aren't typing in a field.

By default it matches **module names**, **Settings sections** and **Resources** entries. The **layers toggle** next to the box widens it to your content: Editor pages, Goals, Calendar events, ToDos, Routines, Reminders, Prompts and Community Docs.

Two things it deliberately does not do: it matches **titles and names only, never bodies**, and it only searches modules you have installed. Results come back grouped by type, prefix matches first; <kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd> navigate them.

## Notifications

The bell in the top bar carries an unread count and opens the **notification inbox**, where [RemindMe](/modules/productivity#remindme) reminders land when they fire, along with anything an inbound [webhook](/modules/productivity#webhooks) redirects there. A notification that fires while you're in the app also slides in as a banner.

Delivery to **email, Telegram, Slack or Discord** goes through the shared [notification channels](/config/settings#notifications) in Settings, where you also decide which modules may push to each one. The inbox itself is always on and needs no setup.
