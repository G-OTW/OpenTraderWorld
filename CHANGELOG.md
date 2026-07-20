# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6] - 2026-07-19

### Added
- **Demo mode** — an opt-in posture (`OTW_DEMO=1`) that turns an instance into a public shared sandbox, so anyone can try the app without installing it. Never enabled implicitly; a normal install is untouched. The API switches to **default-deny**: every request must match an explicit allowlist or it is refused, which means a new module or route stays blocked in demo until someone opens it deliberately (the same philosophy as the MCP catalog). Read-heavy features stay browsable while anything that writes secrets, manages providers or reaches the network is held read-only or closed outright. A public status endpoint drives an in-app sandbox banner with a countdown to the next reset, and the whole database is wiped and re-seeded every 15 minutes. Ships with its own hardened `deploy/demo/` stack (compose file, throwaway env template, `reset.sh`) and a seeded dataset — journal trades, portfolios, watchlists, news feeds, prompts and backtest runs — so the sandbox looks like a used install rather than an empty one.
- Demo: the seeded AI agent runs on a **dedicated free-tier key**, repinned at boot onto a model that key can actually serve that day. The agent reaches its own data through the MCP gateway in-process using a seeded read-only token, while the external MCP endpoint and all token management stay closed.
- Demo: a **welcome disclaimer** that must be acknowledged before exploring — the sandbox runs on a free-tier server with free AI models, so it states plainly that pages and answers can be slow, that AI output may be wrong and is not financial advice, that everything typed is visible to other visitors and wiped every 15 minutes, and that settings and connectors are read-only. Acknowledged once per browser session, so a returning visitor landing on freshly reset state sees it again.
- Demo: the seeded sandbox now covers far more of the app, including a **"Market news" feed** backed by an optional AlphaVantage key (`OTW_DEMO_AV_KEY`) that `--seed-demo` seals into the encrypted feed secrets — never written to the seed SQL or the repo. Left unset, that one feed stays disabled; the Reuters/Fed/ECB RSS feeds need no credential.
- Dashboard: **four new widgets** — Agent (chat with the assistant straight from the dashboard), Economics, Prompts and Watchlist — plus a per-widget configuration panel. Translated across all seven language packs.
- **Agent** — a new module and a sparkles shortcut in the top bar (next to the search) that open a agent chat. Bring your own provider: add one or more of the two supported wire formats — **Anthropic** (Claude Messages API) or any **OpenAI-compatible** endpoint (OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, Gemini's compat endpoint, …) — each with its own base URL, API key (write-only, never shown again, encrypted at rest with the app's master key) and default model. Nothing is vendor-defaulted: the empty state asks you to add a provider of your choice. Configure the assistant's system prompt, active provider/model, max tokens and temperature. Replies stream live (Markdown-rendered, with an optional "Thinking" fold for models that expose reasoning), conversations are saved in a sidebar with rename / delete and one-click **Markdown export**, and you can stop a run mid-stream.
- Agent: **tools over your OTW data.** Attach an MCP token to the assistant and its per-module permission levels (Read / Read+write / Full, set in Settings → AI agents) apply **directly** — the token IS the permission envelope, with no second agent-side gate. Dispatch goes through the same in-process gateway as external MCP clients (catalog allowlist, per-module checks, settings/secrets/wipe exclusions; no shell or filesystem access by construction). Tool calls appear inline as collapsible chips showing the arguments and result. Bounded at 15 tool rounds per run.
- Agent: **memory & skills.** Long-term memory — the assistant saves small durable facts (a preference, a stable detail) that persist across conversations; you browse, edit and delete them all in a Memory tab (nothing hidden). Only the index (slug + one-line description) rides in the prompt; full content is pulled on demand. Skills — reusable Markdown instruction sets you define in a Skills tab; their name + description are always in the assistant's context and it loads the full body on demand when a task calls for it (enable/disable per skill). Plus a **rolling summary**: once a conversation grows large, older turns are automatically compressed into a running summary so long chats stay cheap, keeping only the most recent messages verbatim.
- Agent: **in-chat provider & model switcher.** A compact picker in the chat header shows the active provider · model; open it to switch provider or pick a model from the provider's **live model list** (queried server-side — your key never reaches the browser; free text still works for proxies without a list). Changes save instantly and apply to the next message. An "Advanced parameters (JSON)" field in settings passes any extra request field verbatim to the provider (a `null` value removes a key — e.g. `max_completion_tokens` for newer OpenAI models, or dropping `stream_options`).
- Agent: **per-conversation tools.** Each conversation carries its own MCP token, switchable from a tools dropdown in the chat header (the token set in settings is just the default for new conversations). Two conversations can run with different data scopes side by side; the dropdown notes when the selected token grants write/delete, and quick-links to Settings → AI agents (create/manage tokens) and to the MCP store.
- Agent: **MCP store — connect external platforms.** A new full-page section (Agent → Manage servers) to add remote MCP custom servers by URL. Streamable-HTTP only (nothing ever executes locally), auth values encrypted at rest and write-only, a **Test** button that connects and lists the server's tools, and per-conversation checkboxes in the tools dropdown. External tools are namespaced (`deepwiki__ask_question`), labeled with their server, time-boxed, and an unreachable server degrades to a warning instead of blocking the chat — with an explicit injection warning when combined with write access.
- Agent: **clearer errors and a token badge.** Provider failures now read as a plain sentence instead of raw JSON — a rejected API key, a rate limit (with the provider's retry-after when it sends one), a wrong model/base-URL, or a server hiccup each get their own message, shown in a dismissible banner above the composer. A content-filter refusal or a reply cut off at the token limit is surfaced too, instead of an empty turn. The chat header shows a running **token count** for the conversation (input + output), so you can see what a thread is costing at a glance.
- **Vault** — an encrypted secrets store for the credentials the app needs on your behalf (provider API keys, feed tokens…). Items are encrypted at rest with the app's master key and write-only once saved; a shared picker lets any module reference an item instead of pasting the secret again. News feed secrets also accept inline `{{vault.item}}` placeholders, resolved by the scheduler at poll time, so a feed URL or header can carry a key without ever storing it in the feed config.
- **Watchlists** — a new module: named lists of symbols with live quotes, day change and sparklines, reconciled from public sources (CoinGecko / Yahoo) out of the box.
- Watchlists: **custom quote sources.** Create your own provider connectors (Sources view) with their own names, vault-picked credentials and request limits, kept separate from Historical Data's. A list can pin a connector as its default source, and each symbol can override it (follow list / auto / a specific connector); per-item provider tickers (`BTCUSDT`, `AAPL.US`, …) are derived and editable, and quote failures surface per row. Lists with a custom source unlock **5s–30s refresh intervals**, with an explicit know-your-limits warning pointing at Settings → API rate.
- Portfolios: **per-asset trade currency.** Buys and sells were implicitly USD; each asset now declares the currency its operations are entered in. Spot quotes stay USD — cost basis and realized PnL convert at each operation's own date via the journal FX rates, so historical buys keep their historical rate. Form labels follow the asset's currency, and a popover explains where prices come from.
- **One-command install.** `curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash` — a new root `install.sh` checks Docker is ready, fetches only the `deploy/` directory (no source checkout, no toolchain), and hands off to the guided setup, which pulls the prebuilt Docker Hub images. Options: `--dir` (install directory), `--ref` (branch/tag), `--build` (full source clone + local image build). It refuses to touch an existing install and reattaches the terminal so the setup prompts work under `curl | bash`.

### Changed
- Setup: when volumes from a previous install exist, the "wipe existing data" prompt now defaults to **No** — a headless or absent-minded run can no longer delete the previous database; wiping requires an explicit `y` (declining aborts, since fresh secrets can't open the old database volume).
- **New finance redesign** across the entire frontend: self-hosted Inter + JetBrains Mono, a denser type scale, square corners with no shadows or gradients, separation by hairline rules instead of boxes, and all numerals in mono. The accent is now a single restrained gold, spent only on the active nav item, status dots, the equity curve and keyboard focus — buttons, badges, links and chart series are neutral. Charts drop area fills and vertical gridlines. The default theme accent also moved from indigo to **teal**.
- Backtest: **fixed-quantity sizing now scales with leverage**, matching the retail convention (a leverage of 3 on a fixed size of 1 opens 3 units) instead of leaving the size untouched.
- Settings → Credits is now grouped **by kind of data instead of by module**. The same market-data providers feed quotes, watchlists and historical downloads, so a per-module split repeated most of the list; the new Market data / Economic calendar / Investor portfolios / Instrument reference / FX rates grouping lists every provider the platform can pull from exactly once, configured or not.

### Fixed
- Dashboard: the per-widget configuration gear is reachable in view mode again, and grid filets are drawn as cell outlines instead of overlapping rules.
- Histdata: intraday downloads from Yahoo no longer fail outright when the requested window reaches further back than Yahoo keeps that interval (~30 days for 1m, ~60 for 5m/15m, ~730 for 1h). The start is clamped to the available depth instead of the whole request being refused.
- Demo: the agent composer is no longer blocked on a missing API key. The seeded provider row deliberately carries no key — the shared one lives in the host environment — so the UI now reports the agent as usable when the host supplies it (still only a boolean; the key never leaves the server).
- Journal: cost basis is now genuinely **FIFO** when selected — the setting was accepted but silently computed as weighted average, both server-side and in the trade form's live PnL preview. Capital events on a date with no FX rate queue a pending task instead of quietly skewing invested capital.
- Journal: editing an advanced trade no longer leaves behind exit legs from brackets that were un-triggered.
- Backtest: stop-and-reverse entries were silently skipped under risk-based sizing (the stop price was never passed through). Grid trades now carry their entry fee, so trade stats reconcile with the equity curve. Sizing, exposure and margin checks evaluate marked-to-market equity net of margin already committed, so a portfolio can no longer stack positions past its buying power.
- Backtest: the run report page scrolls on its own instead of being clipped by the app shell (and flows normally when printed).
- TaxCalc: profiles gain a flat-rate override; investing gains with no matching holding-relief tier are taxed as income where the regime defines tiers (US short-term at 24%, not the long-term 15%), and wealth-tax brackets are sorted before slicing so unsorted profiles no longer tax later brackets against the wrong base.
- FX: the catch-up job retries pending dates against the business day on-or-before (ECB coverage from 1999) and re-fetches dates stored with partial coverage.
- Quant: a capital-`M` timeframe suffix now reads as months, closing the `1M` month-vs-minute annualization trap; the efficient-frontier chart no longer clips its y-axis label.
- Agent: only one run at a time per conversation, so interleaved saves can't mispair tool calls with their results; the rolling summary now cuts at a user-message boundary; OpenAI-compatible providers that resend a full tool name no longer end up with it concatenated.
- Notification channels: messages are capped to each platform's limit (Discord 2000, Telegram 4096, Slack 40k) so long payloads truncate instead of failing outright.

### Security
- Login now throttles failed attempts (10/min), removing an unthrottled password oracle in web mode.
- The session cookie carries `Secure` in HTTPS network modes (`lan_https`/`web`) and a `Max-Age` matching the 7-day server TTL; expired sessions are purged on each new login.
- Agent: external MCP tool names are deduplicated within a server too, so a name-truncation collision can no longer dispatch to the wrong tool.

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

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.6...HEAD
[0.0.6]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
