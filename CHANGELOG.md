# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.5] - 2026-07-14

### Added
- **Global search** — a search bar in the centre of the top bar (focus it from anywhere with ⌘K / Ctrl K, or `/`). By default it matches names/titles across your installed modules, Resources items and Settings sections; a layers toggle in the field widens it to content titles — Editor pages, Goals, Calendar events, ToDos, Routines, Reminders, Prompts and Community docs (titles only, never bodies). Results appear as you type, grouped by type with the matched text emphasised; arrow keys + Enter (or a click) jump straight there.
- Journal: **PnL Calendar** — a new sidebar view showing the classic month-grid heatmap of daily realized PnL. Each day is a green/red cell whose colour intensity scales to the month's biggest day, with its net PnL and trade count; a weekly-total column runs down the right edge and the header sums the month (net PnL, green/red day split, trades, win rate). Navigate months (or jump to today), and click any day to jump straight to that day's trades. Respects the selected category and the journal's display currency (FX-converted); read-only, no schema change. The daily buckets are also exposed to AI agents through the MCP gateway.
- Journal: **exports** — a new Export button in the module header opens a dialog with two outputs. **Trades CSV**: a spreadsheet-ready file of every trade field (typed columns, computed gross/net PnL, category/strategy names, legs as JSON for advanced trades), scoped to the current category and to all time, a week or a month. **Performance report**: a weekly or monthly report — net PnL, win rate, expectancy, profit factor, fees, avg win/loss, best/worst trade, max drawdown, the equity curve, per-strategy / per-category breakdown tables and the trade log — downloadable as **PDF or Markdown**, with all amounts FX-converted to the journal's display currency.
- **Shared report engine** — one document model (stat cards, tables, bullets, time-series charts) with two renderers: deterministic Markdown and a dependency-free vector PDF (stat-card grid, zebra tables with page breaks, line charts with baseline and axis ticks, page footers). The Journal report and the Backtest run report are both built on it, and saved backtest runs gain a server-rendered **`report.pdf`** endpoint alongside the existing `report.md`.
- Quant Tools: **Monte Carlo** — resample a saved backtest run's realized trade sequence into thousands of alternate equity paths to see how much of the result was luck of ordering. The run is replayed to regenerate its exact trades, then resampled by IID bootstrap or streak-preserving block bootstrap; the result shows an equity fan (p5–p95 / p25–p75 bands with the median and the actual curve), the final-equity and max-drawdown distributions, and headline stats — **risk of ruin** (share of paths that breach a chosen drawdown-from-start threshold), median max drawdown, probability of loss and median final equity. Choose iterations, resampling method, the ruin threshold and the horizon.
- Quant Tools: **Seasonality** — month × weekday heatmap plus by-month, by-weekday and (intraday only) by-hour strips of a dataset's period returns, switchable between mean return and volatility. Positive returns read green, negative red; hover any cell for the value, sample count and win rate.
- Webhooks module: **inbound webhooks** — private URLs any external service can POST to (alerting platforms, monitors, scripts…); each received payload is redirected to a module of your choice, starting with RemindMe (payload → in-app notification, also pushed to your enabled channels). Endpoints carry a 256-bit token in the URL (hashed at rest, shown once at creation), can be enabled/disabled, and keep a per-endpoint delivery log. Payloads are parsed liberally (plain text or JSON with loose field names). The page warns when the current network mode isn't reachable from the internet and points to Settings → Network or a tunnel.
- MCP: AI agents can now **create and manage backtest strategies and custom indicators** — the strategy and indicator creator endpoints (create / update / delete) are exposed through the gateway, gated by read+write permission on the Backtest module. Agents can author a strategy, read it back, and run it end to end.
- Central **API request limits** — declare a limit on any external API the app calls for you and watch it fill up. On a news API source: a checkbox in the feed form sets a request cap (or unlimited-but-tracked) per minute/hour/day/week/month, and the source card in the left pane grows a small progress bar (used / total, colored as it fills, reset each period). On a Historical Data connector: the same limit declaration, displayed as a pie gauge in the connector list and the download picker. Counting happens on real requests (each feed poll, each provider call the download worker makes, retries included); limits are informative only — nothing is throttled.
- Histdata: **named provider connectors** — create as many instances of the same provider as you want, each with its own name, its own encrypted credentials and its own request limit. Existing credentials migrate automatically to a default connector per provider; downloads remember which connector they ran through.

### Changed
- MCP: **finer-grained permissions** — module access now has three levels instead of two: **Read**, **Read + write**, and a new **Full (read + write + delete)**. Delete is no longer bundled into read+write, so an agent can be allowed to create and edit a module's data without being able to delete any of it. Set it per module or apply one level to all modules at once (All read / All read+write / All full). A `DELETE` endpoint is now hidden from an agent's catalog unless its token has Full on that module. **Note:** existing tokens with read+write no longer permit delete — re-edit a token and set the module to Full to restore it.
- MCP: `otw_catalog` is now **two-level** to cut token use — calling it with no argument returns a compact index of accessible modules (label, access level, endpoint count), and passing a `module` lists that module's endpoints. Agents drill into only what they need instead of loading the full catalog.
- Backtest: the result chart stays readable on runs with many trades — entry/exit markers and SL/TP lines are hidden when the visible window is too dense (a "zoom in to see individual trades" hint appears) and reveal themselves as you zoom in. Multi-asset (portfolio) runs get a **dataset picker** at the top-left of the chart to switch which asset's candles are shown, and the trades table is now **paginated** (100 per page).
- Histdata: the download form's provider dropdown is replaced by a **searchable connector picker** — type to filter, each row showing the connector's readiness dot or usage pie, its name, provider and paid tag. The Settings tab is now a full connector manager (add / rename / delete, per-connector credentials, limit editor with live usage and reset time).

## [0.0.4] - 2026-07-11

### Added
- Backtest: **portfolio backtesting** — run one strategy across several datasets on a merged clock, with an alignment preview (overlap window, warm-up bars, per-asset missing bars) and a per-asset breakdown of trades, PnL, fees and exposure.
- Backtest: **expert mode** — a full-screen builder for named strategies and custom node-graph indicators (multi-step, each step referencing earlier ones), with duplicate and search.
- Backtest: **grid strategy** — a ladder of levels that buys low and sells one level up (long / short / neutral), sized per level or by splitting a total budget; reports fills, round trips and end inventory.
- Backtest: **out-of-sample split** — runs the strategy on an in-sample head and an out-of-sample tail and shows the two stat blocks side by side to expose overfitting.
- Backtest: advanced sizing — **risk-per-trade** (stop-based), **fractional Kelly** (windowed, capped), and **equity tiers**; plus **portfolio limits** (max open positions, max total and per-asset exposure).
- Backtest: cost & execution realism — **slippage** (ticks or % of price), **funding** estimate for perps, **circuit breakers** (max daily loss / max drawdown halt), and instrument profile (tick size, lot step, min quantity, contract multiplier).
- Backtest: per-trade **MAE/MFE** (max adverse / favorable excursion) and a filterable trades table.
- Backtest: downloadable **Markdown report** per saved run.
- Prompt Store module: a searchable library of reusable prompts shown as a grid of vignettes (name, tags, last-saved), with tag filtering, thumbs up/down rating and quick filters, duplicate, and per-prompt version history with rollback.

### Changed
- Backtest: the custom-indicator builder now chains **indicators onto any source** — each step applies an indicator to a price field or to an earlier step's output (e.g. `HullMA` of an `RSI`, `MACD` of an `RSI`), with named steps and free-form math **formula steps** (`@volume / SMA(@volume)`) referencing steps by name.
- Backtest: **stop-loss and take-profit can be toggled on/off** independently with a checkbox, rather than disabled by entering 0.
- Design enhancement.

### Fixed
- Doc submissions from the editor now work out of the box: the submission relay no longer requires `DOC_SUBMISSION_TOKEN`.
- Doc submissions: uploaded images (`/api/files/…`) are now inlined as `data:` URIs before relaying.

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

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.5...HEAD
[0.0.5]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
