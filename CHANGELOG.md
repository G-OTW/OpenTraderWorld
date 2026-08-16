# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8] - 2026-08-16

### Added
- Accounts: a forgotten password is recoverable from the **host shell** — one command prints a one-time password, and the sign-in page's "Forgot password?" link spells out the steps.
- Assistant: a **floating chat** on every page — the agent one click away, over the same conversations, personas and providers as the Agent module, already aware of the page you asked from.
- Dashboard: Subscriptions and Net worth widgets — the monthly total above the next renewals, and the current net worth with its change and a sparkline over 6, 12 or 24 months.
- Watchlists: a list's description is edited in place from its header instead of only in the create form.
- Resources: a **gallery** display with a thumbnail per bookmark — uploaded, pasted as a URL, or fetched from the link's own social preview, with an initials tile when there is none.
- Notifications: one **shared channel list** (email, Telegram, Slack, Discord) in Settings, reachable from every module, where you choose which modules may send to each channel.
- Watchlists: **price alerts** per symbol — a level, or a move in % or $ measured from now or over a rolling window — firing on the server even with the page closed, to the notification channels you pick per alert.
- Portfolio tracker: the description is editable after creation, next to a foldable **investment thesis** note kept with the portfolio.
- Portfolio tracker: import an operations ledger from any broker export or spreadsheet — columns are detected, you say what each symbol is, and each import is one revertible batch, de-duplicated per portfolio.
- Journal: import a trade book from any broker export, journal or spreadsheet (CSV/TSV/JSON) — columns, delimiter, decimal and date conventions are detected, and unidentified columns stay unmapped rather than guessed.
- Journal: the import previews real trades before writing anything, cross-checks the file's own P&L against the computed one, and re-renders on every mapping edit.
- Journal: rows can be round trips or executions; fills are grouped per instrument into positions, and what stays open at the end is imported as an open trade.
- Journal: each import is one revertible batch, de-duplicated per category, so re-importing the same file changes nothing.
- Journal: statement exports stacking several tables in one file are read, with a picker to switch table or read the file flat.
- Journal: the import asks for the point value of each ticker it found, saved with the mapping.
- Journal: mappings can be saved, are matched to the next file by header fingerprint, and corrected columns are remembered.
- Time tracker: the breakdown chart switches between hours and cost, and can stack each bar per timer in the timer's own colour.
- Time tracker: a ranking of timers under the totals, ordered by hours or cost, expanding into the individual runs behind each one.
- MyWealth: the net-worth chart draws as a line or as bars, one bar per month or year.
- Goals: a colour per goal, on its card and progress bar, with roomier metric rows that scroll past five instead of stretching the form.
- Goals: a statistics page breaking goals down by status, deadline, time remaining and metrics reached — click any slice to list the goals behind it.
- ToDo: a due-date timeline beside the list, zooming from years to months to days to filter tasks, hidden or shown from the header.
- ToDo: tasks with notes unfold them on click instead of showing them on every row.
- Routines: a Templates page to build reusable checklists, with categories, rich notes and a link per step.
- Routines: schedules beyond weekdays — every N days or weeks, days of the month, the 1st or last Thursday, a single date, all bounded by an optional active period.
- Routines: consistency now counts the days you mark yourself, over a week, month or year, on a calendar where a day is green (routine followed) or amber (acted without it).
- Routines: tick a whole routine or a whole category at once, and fold categories away on the board.
- Mindset: a Templates page to build reusable check-ins, with categories, rich notes and a live preview of every prompt as you edit it.
- Mindset: several check-ins per day instead of one fixed pre-mortem and post-mortem, each placed before, during or after the session.
- Mindset: consistency counts the days you mark yourself, over a week, month or year, on the same calendar the routines board uses.
- Mindset: history moves to its own page, with the trend of every 1–5 prompt over the last month to year.
- RemindMe: a weekly reminder picks the days it fires on, with weekdays/weekend/every-day shortcuts and a preview of the next dates.
- RemindMe: a reminder can carry a link, opened from the list, its notification and the toast, and included in email/Telegram/Slack/Discord messages.
- Visualization: drawing tools beside the chart — trend line, horizontal, vertical, rectangle, Fibonacci, text, long/short position and a measure — with per-object style, OHLC magnet and per-instrument persistence.
- Visualization: **quick backtest** — click the chart to open a position and again to close it, long press for side and size, click an arrow to flip it; pyramiding and partial closes included.
- Visualization: a trade book under the chart listing the quick session's trades with its P&L, win rate and average R, and a reset.
- Visualization: undo takes back the last drawing or the last quick-backtest order, from the drawing rail, the panel, or Ctrl/Cmd+Z.
- Visualization: a Backtest button that saves the instrument if needed and opens the backtest module on it.
- Visualization: a stored instrument keeps its indicators and drawings server-side and reopens as you left it from any browser; an optional switch stores an instrument the first time you chart it.

### Fixed
- FinanceDatabase: updating the catalog keeps your favorites — one no longer listed stays visible, marked, and comes back if a later update lists it again.
- Mailbox: the page keeps its padding and the mail grid fills the width again.
- MyWealth: assets in another currency no longer drop out of the net-worth chart for dates before the exchange-rate history starts.
- MyWealth: the yearly chart no longer samples a year end in the future, which flattened the whole curve to zero.
- Visualization: the crosshair no longer disappears every time a live bar lands.
- Visualization: a bar closing no longer resets a price scale you set by hand, nor the slice of history you were looking at.
- Visualization: the scale labels the level under the cursor instead of snapping that label to the hovered bar's close.
- Visualization: the quick backtest panel resizes, sizes in units, contracts or dollars, and reads like a strategy tester: trades, P&L curve with excursions, and stats you can hover.
- Visualization: candles, volume and indicators no longer paint outside their pane while the chart is dragged.
- Visualization: a micro-cap's prices are written 0.0₅4549 — the subscript counts the zeros — instead of a scale of identical 0.0000 labels.
- Visualization: the toolbar and the quick-backtest header line up on one height, and closing the quick backtest actually closes it.

### Changed
- MyWealth: type and category filter several values at once and now scope the chart above as well as the table; long cells show their full value on hover.
- Journal: a category's colour is picked in its edit modal; export only shows on the views that hold trades.
- Subscriptions: a bell on the next-billing cell creates a reminder seeded from the row, and dates now follow the language picked in the app rather than the browser's.
- FinanceDatabase: the install screen now states what the catalog holds, its size and that it works offline afterwards, with a progress bar during the import.
- UI: every picker in the app is now the app's own dropdown rather than the OS one, marking the current value in colour, and filter menus no longer open behind a table header.
- Community docs: browse docs as cards or as a list, and category cards now preview the docs they hold.
- Data connectors: the buttons on a connector all share one style and height instead of three.
- Managers' Portfolios: the holdings modals are wide enough for the whole table, and the refresh button spins until the table has actually reloaded.
- Agent: model and prompt are picked on a searchable screen with a preview, confirmed by a button — a dialog on the Agent page, a view inside the floating assistant, whose selectors now sit on one row under the title.
- Editor: assorted improvements — formatting, colours, images and databases.
- Visualization: the chart takes the whole page — instrument and OHLC written on it, one line per indicator with its controls on hover, and the instrument picker moved to a modal opened from the symbol.
- Visualization: volume is drawn at the bottom of the price pane instead of a pane of its own, windowed indicators are titled over the pane that draws them, and the candles start right under the header.
- Visualization: dragging the chart pans both axes — time sideways, price up and down — and the autosave toggle moved out of the settings popover onto the toolbar as a switch.
- News: assorted improvements — dashboard bar, source colours and filters.
- Mailbox: the module now reads in German, Spanish, Italian, Portuguese and Chinese instead of falling back to English.

## [0.0.7] - 2026-07-25

### Added
- Agent: five built-in personas (Quant, Portfolio Manager, Day Trader, Researcher, Financial Analyst), switchable mid-conversation, each with its own prompt and skill shelf.
- Agent: 19 built-in skills — procedures loaded on demand for backtesting, risk metrics, journal audits, research briefs and more.
- Agent: a persona editor — create, duplicate, edit, reset or delete a persona; deleting one keeps its conversations.
- Agent: export and import personas and skills as JSON, skill bodies included.
- Agent: write confirmation — the run pauses and shows the exact call before changing your data; deletions always ask, read-only endpoints never do.
- Agent: provider, model and simulation budget set per conversation rather than globally.
- Agent: a wide mode for the conversation, for the tables the assistant produces.
- Agent: memories record which persona wrote them.
- Backtest: date-windowed runs — `from`/`to` restrict the simulated span and are stored with the run.
- Backtest: `POST /api/backtest/sweep` runs a parameter grid server-side (up to 4 axes, 64 trials) and reports a deflated Sharpe alongside the trials.
- One centralized broker for market-data connectors, granted per module, reachable from Settings, `/connectors` and every module that reads market data.
- Visualization: chart any instrument a connector serves, stored or not; nothing is written until you press Save, and stored bars are never downloaded twice.
- Visualization: live streaming is addressed by instrument and stores nothing until you save it into a dataset.
- Visualization: a Data tab with one symbol search over every connector the chart may use, plus Recent and Stored shelves.
- Visualization: a last-price tag and a left/right price scale in the chart settings.
- Visualization: a window that comes back short names why — credential, quota, rate limit, unknown symbol or history depth.
- Visualization: a Custom tab on the indicator picker — build or pick a custom indicator from the same library the backtester uses, and draw it on the price or in its own pane.
- Agents can read the connector list and pull bars through `/api/histdata/preview`, `/api/histdata/symbols` and `/api/histviz/series`.
- Demo: HTTPS on a real hostname (`OTW_DOMAIN`), a pinned image tag (`OTW_IMAGE_TAG`) and a restricted bind address (`OTW_DEMO_BIND`).
- Install: `--yes` / `OTW_ASSUME_YES` accepts every prompt default, `--wipe-volumes` clears stale data volumes.

### Changed
- The per-module provider settings are gone; both are replaced by a Connectors button onto the shared broker, with existing grants untouched.
- Agent: the chat always opens with a conversation ready, and the tools dropdown stays visible even with nothing configured.
- Agent: conversation titles are cut to 38 characters on a word boundary.
- Visualization: the chart is driven by hand instead of by ECharts' roam — 1:1 drag panning, per-device wheel zoom, draggable scales, keyboard equivalents and room after the last bar.
- Visualization: changing chart type keeps the zoom, and indicators are computed once per render instead of three times.
- Visualization: the legend reads values at the crosshair, the timeframe picker is a dropdown offering every bar size the connector supports, and switching timeframe keeps the dates on screen.
- Visualization: history loads on demand, 1500 bars per click, instead of fetching on scroll.
- Visualization: prices are written with the instrument's own precision, from whole points to eight decimals, on the axis, the tags and the legend.
- Dropdowns past eight entries open with a search field that filters as you type, keeps the arrow keys working and highlights what matched.
- Visualization: one fullscreen instead of two.

### Fixed
- Agent: sending a second message while a reply was streaming deadlocked the backend; a double-send is now a plain error.
- Agent: declining a write is reported as a refusal instead of a timeout, and a refused simulation is no longer charged to the budget.
- Agent: the built-in backtest skill taught a configuration that closed every trade after one bar; it now carries explicit exits and the engine warns when it sees the pattern.
- Agent: the default assistant carries the same safety rules as the personas, and knows the current date.
- Agent: the module index rides in the prompt and older tool payloads are trimmed, so a run stops paying for the same catalog every turn.
- Agent: the memory index no longer grows the prompt without bound, `memory_delete` can prune a full store, and the assistant can no longer overwrite a memory you wrote by hand.
- Agent: hitting the tool-call limit ends on a conclusion; a model that cannot call tools replays the turn without them.
- Agent: unparseable tool arguments are reported as such, and the chat shows a status line for the whole run instead of going blank during tool calls.
- Agent: Monte Carlo is exposed to the gateway, with a summary view that returns 613 bytes instead of 40 KB.
- Agent: the backtest skill documented a wrong field name for constant operands; all operand kinds and comparison operators are now documented.
- Install: `curl | bash` no longer takes every default silently on EOF, and a partial install resumes instead of refusing.
- Visualization: the wheel zooms the chart again.

## [0.0.6] - 2026-07-19

### Added
- Demo mode (`OTW_DEMO=1`) — an opt-in public sandbox on a default-deny API, wiped and re-seeded every 15 minutes, with its own hardened stack, seeded dataset and welcome disclaimer.
- Demo: the seeded agent runs on a dedicated free-tier key over an in-process read-only MCP token, and the sandbox includes a market-news feed from an optional AlphaVantage key.
- Agent — a new module and a top-bar shortcut: bring your own Anthropic or OpenAI-compatible provider, streamed replies, saved conversations with Markdown export, and configurable prompt, model, tokens and temperature.
- Agent: tools over your OTW data — attach an MCP token and its per-module permissions apply directly; tool calls show inline as collapsible chips, bounded at 15 rounds per run.
- Agent: long-term memory, reusable skills, and a rolling summary that compresses older turns.
- Agent: an in-chat provider and model switcher backed by each provider's live model list, plus an advanced-parameters JSON field.
- Agent: per-conversation MCP tokens, so two conversations can run with different data scopes.
- Agent: an MCP store for external Streamable-HTTP servers — encrypted auth, a Test button, namespaced tools and per-conversation selection.
- Agent: provider failures read as plain sentences, and the header shows the conversation's running token count.
- Vault — an encrypted store for the credentials the app uses on your behalf, referenced by any module and resolvable inline in feed URLs as `{{vault.item}}`.
- Watchlists — named symbol lists with live quotes, day change and sparklines, custom quote connectors, per-symbol source overrides and 5s–30s refresh.
- Dashboard: Agent, Economics, Prompts and Watchlist widgets, plus a per-widget configuration panel.
- Portfolios: per-asset trade currency, with cost basis and realized PnL converted at each operation's own date.
- One-command install: `curl -fsSL …/install.sh | bash`, with `--dir`, `--ref` and `--build`.

### Changed
- Setup: the "wipe existing data" prompt defaults to No, so a headless run cannot delete the previous database.
- A finance redesign across the frontend: self-hosted Inter and JetBrains Mono, denser type, square corners, hairline rules, mono numerals and a restrained gold accent; the default theme accent moves to teal.
- Backtest: fixed-quantity sizing scales with leverage.
- Settings → Credits is grouped by kind of data instead of by module.

### Fixed
- Dashboard: the per-widget gear is reachable in view mode, and grid filets no longer overlap.
- Histdata: intraday Yahoo downloads clamp to the available depth instead of failing outright.
- Demo: the agent composer is no longer blocked on the deliberately keyless seeded provider row.
- Journal: FIFO cost basis is actually FIFO, and a capital event with no FX rate queues a pending task.
- Journal: editing an advanced trade no longer leaves exit legs from un-triggered brackets.
- Backtest: stop-and-reverse entries work under risk-based sizing, grid trades carry their entry fee, and exposure checks account for margin already committed.
- Backtest: the run report scrolls on its own and prints normally.
- TaxCalc: a flat-rate override per profile, income treatment for gains with no holding-relief tier, and wealth brackets sorted before slicing.
- FX: the catch-up job retries pending dates against the business day on-or-before and re-fetches partial coverage.
- Quant: `1M` reads as months, not minutes; the efficient-frontier chart no longer clips its axis label.
- Agent: one run at a time per conversation, the rolling summary cuts on a user message, and full tool names are no longer concatenated.
- Notification channels: messages are capped to each platform's limit instead of failing.

### Security
- Login throttles failed attempts (10/min).
- The session cookie carries `Secure` in HTTPS network modes and a `Max-Age` matching the server TTL; expired sessions are purged on login.
- Agent: external MCP tool names are deduplicated within a server too.

## [0.0.5] - 2026-07-14

### Added
- Global search in the top bar (⌘K / Ctrl K, or `/`) over modules, Resources and Settings, widened on demand to content titles across Editor, Goals, Calendar, ToDos, Routines, Reminders, Prompts and docs.
- Journal: a PnL calendar — a month grid of daily realized PnL with weekly totals and a month header, clicking through to that day's trades.
- Journal: exports — a trades CSV and a weekly or monthly performance report as PDF or Markdown, FX-converted to the display currency.
- A shared report engine (stat cards, tables, charts) with Markdown and dependency-free PDF renderers, behind both the journal report and the backtest run report.
- Quant Tools: Monte Carlo — resample a saved run's trades into thousands of paths, with an equity fan, risk of ruin, median max drawdown and probability of loss.
- Quant Tools: seasonality — month × weekday heatmap plus by-month, by-weekday and by-hour strips of period returns.
- Webhooks: inbound endpoints any service can POST to, each redirecting its payload to a module (RemindMe first), with a hashed token, an enable switch and a delivery log.
- MCP: agents can create and manage backtest strategies and custom indicators.
- Central API request limits — declare a cap on any external API the app calls, shown as a progress bar per news source and a pie gauge per connector; informative only, nothing is throttled.
- Histdata: named provider connectors, several per provider, each with its own credentials and limit.

### Changed
- MCP: three permission levels per module — Read, Read + write, and Full (adds delete). Existing read+write tokens no longer permit delete; set Full to restore it.
- MCP: `otw_catalog` is two-level — a module index by default, endpoints on request.
- Backtest: trade markers hide on dense windows and reveal on zoom, portfolio runs get a dataset picker, and the trades table is paginated.
- Histdata: a searchable connector picker in the download form, and a full connector manager in the Settings tab.

## [0.0.4] - 2026-07-11

### Added
- Backtest: portfolio backtesting — one strategy across several datasets on a merged clock, with an alignment preview and a per-asset breakdown.
- Backtest: expert mode — a full-screen builder for named strategies and multi-step custom indicators.
- Backtest: grid strategy — a ladder of levels, long, short or neutral, sized per level or from a total budget.
- Backtest: out-of-sample split, showing in-sample and out-of-sample stats side by side.
- Backtest: risk-per-trade, fractional Kelly and equity-tier sizing, plus portfolio exposure limits.
- Backtest: slippage, perp funding, circuit breakers and an instrument profile (tick size, lot step, min quantity, multiplier).
- Backtest: per-trade MAE/MFE and a filterable trades table.
- Backtest: a downloadable Markdown report per saved run.
- Prompt Store — a searchable library of reusable prompts with tags, ratings, duplication and version history with rollback.

### Changed
- Backtest: custom indicators chain onto any source — an indicator over a price field or an earlier step, with named steps and free-form formula steps.
- Backtest: stop-loss and take-profit toggle independently instead of being disabled with a 0.
- Design enhancements.

### Fixed
- Doc submissions from the editor work without `DOC_SUBMISSION_TOKEN`.
- Doc submissions: uploaded images are inlined as `data:` URIs before relaying.

## [0.0.3] - 2026-07-07

### Changed
- Updates no longer reset the operator's network mode: `network.env` is no longer tracked by git, so `git reset --hard` during an update leaves it untouched.

### Fixed
- Chart: drag-panning no longer gets stuck after a few pixels.
- Settings → Update app now shows the correct update commands (`git reset --hard` + image pull); the previous `git pull` fails on force-pushed releases.

## [0.0.2] - 2026-07-07

### Added
- Prebuilt Docker images and image-based Compose install (multi-arch).
- Documentation site with in-app update check.

### Changed
- Default network bind to localhost.

### Fixed
- findb: treat empty `FINDB_ARCHIVE_URL` as unset.

## [0.0.1] - 2026-07-06

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.8...HEAD
[0.0.8]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
