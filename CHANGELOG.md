# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-28

### Added
- Charts: the range bar shows the current time and its UTC offset, in the chart's timezone.
- Charts: **entry and exit markers** show the account's fills at their price and time (last 30 days, 7 on IBKR), turned on in the Trading settings.
- Brokers: **live trading** opens on Binance spot, Binance USDⓈ-M Futures and Interactive Brokers, their order paths now rated E4.
- Brokers: a position limit can be set as a value in the account's currency instead of a size.
- Charts: the quick bar sizes a crypto order in the base asset or as an amount of the quote, and a Binance USDⓈ-M order in contracts or an amount.
- Charts: the Account tab shows the free and locked balance of both assets of the pane's Binance spot pair.
- Charts: orders the broker never received are listed in the Orders tab, to send again or dismiss.
- Charts: a trailing stop is drawn where it trails now, with its start price until reached, and the Orders tab shows the same level.
- Brokers: **live trading** is turned on per account, with its size, position and loss limits set; each broker's order path shows its level, and live orders open only at E4.
- Brokers: a position that moves without fills to explain it halts the account until you acknowledge it.
- Brokers: the price collar refuses a chart price older than a minute.
- Brokers: a dry run checks an order at the broker without placing it (Binance test route, IBKR what-if).
- Brokers (IBKR): prices are checked against the contract's tick ladder, an unmapped IB answer leaves the order unknown and alerts you, and a live account's first order is dry run.
- Brokers: **risk limits** per trading account (size, value, position, price collar, order rate, open orders, loss per period, duplicates) refuse an order before it leaves, and a **global kill switch** cancels every working order and halts every account until the next hour, day or week, or until lifted with your password.
- Brokers: every order is stored under its id before it is sent, so an id goes out once per account across restarts, an order whose answer was lost is looked up instead of resent, and new orders on its instrument wait until it is settled.
- Brokers: trading accounts are reconciled with the broker on start and on a cycle: missed fills and cancels take the broker's state, orders placed elsewhere are noted, and position drift is logged and notified.
- Brokers: Binance spot and USDⓈ-M accounts follow the venue live, so a chart's orders and futures positions update as Binance pushes them.
- Brokers: Binance orders are checked against every venue filter (notional, price band, market lot, open-order caps) before sending, with the nearest valid values named.
- Brokers: Binance USDⓈ-M brackets and OCO pairs are linked again after a restart, and a dead man's switch cancels their orders if OpenTraderWorld goes away.
- Brokers: Binance order bursts over the account limit are refused with the time to retry, and a live key is checked (trading on, withdrawals off, IP restricted) and the first order dry run before anything is sent.
- Charts: the positions tab shows Binance USDⓈ-M leverage and margin mode, and the Binance spot account tab shows USDT cash and, on live, the wallet value.
- Brokers (IBKR): an order size under the contract's minimum or off its step is refused before sending.
- Paper trading: prices, sizes, fees and cash are booked as exact decimals, so a position closes to exactly flat and a price is checked against its tick with no rounding slack.
- Charts: the order ticket gains a **Reduce only** switch on brokers that take it (Binance USDⓈ-M Futures).
- Charts: the order ticket offers only what the account's broker takes, and gains trailing stops, MIT and LIT, FOK, post-only, iceberg, trigger price, close-position, outside-RTH and IBKR's Adaptive algo where declared.
- Charts: the ticket sizes an order in units, as an amount of money or as the money at risk to the stop loss, and its exits take GTC or Day.
- Charts: **Protect with exits** opens the ticket with a take-profit and stop-loss OCO for an open position, and closing a position cancels its exits.
- Brokers: Binance spot links OCO pairs and a limit entry's exits as order lists, and Binance USDⓈ-M links OCO pairs and follows hedge mode.
- Brokers: paper accounts fill trailing stops, MIT, LIT, FOK, reduce-only and post-only orders.
- Brokers: Binance USDⓈ-M Futures takes a take-profit and stop-loss with the entry, sent reduce-only at once, and the one left is cancelled when the other fills or the entry is cancelled unfilled.
- Charts: alerts gain **closes above** and **closes below**, which fire on each bar that closes past the level instead of only on the crossing.
- Charts: orders from the ticket, the quick bar and the + are sent to any broker account with trading on, and working orders move or cancel from their line on the chart.
- Charts: positions and working orders of trading accounts show on their charts, and the order panel gains Positions, Orders and Logs tabs filtered by broker.
- Charts: positions close from their ✕ on the chart, their price-scale tag or the Positions tab, which shows the result, entry time and entry fees.
- Charts: hovering an order of a bracket draws its links, dashed green to the take-profit and dashed red to the stop-loss.
- Charts: a **Trading** tab in chart settings sets how positions and orders are drawn: result in percent or amount, full or short line or a price-scale tag, side and size shown or not.
- Charts: a working order opens in the ticket by double-clicking its line or from the Orders tab, to change its prices, size or time in force in place.
- Brokers: every order sent is kept in an append-only audit log, with per-account order metrics and a **Cancel all orders** button in Settings.
- Brokers: an optional **trading lock** asks for your authenticator code once after each sign-in before orders go out on any account, and removing two-factor turns trading off everywhere.
- Brokers: a **Paper trading** account fills orders from any chart on its live price, with your own balance, commissions, slippage and leverage, and its fills import into the journal like a real broker's.
- Charts: a future's tick size, point value and currency fill in from the open-source QuantConnect Lean contract specifications, refreshed weekly, when its instrument settings leave them empty.
- Brokers: paper futures tie up the margin per contract set in their instrument settings, and an order on one with no margin offers to set it or to use the contract's full value.
- Charts: an **Account** tab in the order panel shows each trading account's equity, cash, results, margin and available funds, the same way for brokers and paper accounts.
- Charts: drawing tools gain a horizontal ray, a parallel channel, a Fibonacci extension and a callout.
- Charts: the time scale labels round calendar steps and names only what changed (year, month, day, hour), so it no longer repeats full stamps.
- Charts: a file of bars opens straight on the chart from the symbol picker, shown for the session only unless **Keep in Historical Data** is ticked or it is saved later.

### Fixed
- Charts: vertical grid lines sit under their time label and the crosshair instead of between two bars.
- Charts: candles keep their bodies on long histories, and the chart fits its width, so a wide screen shows more bars instead of wider ones.
- Brokers (IBKR): a closed position no longer comes back on the chart for up to 30 seconds after the exit, and an open order is listed once.
- Brokers (IBKR): a trailing stop shows at its stop level instead of its trailing distance.
- Brokers: moving one order of a Binance spot OTO list keeps the others, and changes sent close together on one instrument run one after the other.
- Brokers: a Binance USDⓈ-M close-position stop moves without a false minimum size refusal.
- Brokers: a Binance spot position limit counts the base asset you hold.
- Charts: an order sized as an amount is rounded down to the instrument's lot step.
- Charts: dragging a Binance spot order line no longer shows the order as cancelled and loses its line.
- Brokers: a Binance order whose answer was lost and could not be looked up is followed as unknown instead of stored as refused, and a rate-limit refusal says how long to wait.
- Brokers: a fill read twice no longer raises a false position drift, and a Binance USDⓈ-M bracket on a flat account is refused before anything is sent.
- Charts: the quick bar's Buy button no longer starts a drag when the bar stands on a side.
- Charts: sell orders and short positions stay red when the bearish candles use another colour.
- Charts: candles are drawn sharp on every screen, with even widths and no blurred edges, whatever the display's pixel density.

### Changed
- Charts: switching timeframe keeps the end date in view and never shows fewer candles than a fitted chart.
- Charts: auto scale fits the visible bars and price studies tightly with fixed margins, the same in linear and log, and stays on through a sideways pan.
- Charts: deleting a custom indicator or a script from the library removes it from every chart, saved workspace and template.
- Brokers: the trading lock is optional for live trading; with it off, the account shows a reminder.
- Brokers: Binance reads only the traded instrument's terms, compressed, instead of the whole instrument list.
- Brokers: Binance orders go out faster, their filters read once with the instrument list and their checks read together.
- Brokers: a Binance USDⓈ-M stop change places the new stop before cancelling the old one, so a stop always works, and the changed stop gets a new id.
- Brokers: cancelling an order that already filled is no longer an error at Interactive Brokers and Binance; the order shows as filled.
- Charts: the indicator sheet is redesigned, with a named header, one row per setting with steppers, an appearance table for colour, width and line style, and Cmd/Ctrl+Enter to add.
- Mobile: every module fits phone and tablet screens, with tables as cards or local scroll, tap controls instead of drag, and reachable actions; desktop is unchanged.

## [0.0.17] - 2026-10-20

### Added
- Agent: a **Follow latest** control to resume live scrolling after reading older messages.
- Settings: a **Show the floating Agent button** option, enabled by default and saved across sessions.
- Security: **Remember me** on sign-in, with a browser-session option and sessions lasting up to seven days.
- Visualization: **per-chart settings** behind a cog on each chart, with a full sheet for candles, scales, background, grid, crosshair and last price line, and Apply to all charts.
- Visualization: new chart types: hollow and volume candles, line with markers, step line, area, HLC area, baseline, columns, high-low, Heikin Ashi, line break, Kagi, point & figure and range.
- Visualization: **custom timeframes** (7m, 10m, 3h…) built from the provider's own bars; saving one stores the bars it is built from.
- Visualization: an indicator's source can be another indicator or a **formula** over price and volume, with favourite formulas.
- Visualization: A (auto scale) and L (log) switches on the value scale, and a button back to the latest bar.
- Settings: search and A-Z / Z-A sort in Data connectors.
- Custom indicators: a Pine Script v5/v6 compatible engine that runs most public indicators, the base for pasting TradingView scripts (v1, more planned).
- Visualization: **Pine Script v5/v6 compatible indicators**: paste a script in the indicator sheet to plot it with its inputs, warnings and per-instrument settings, saved in the shared indicator library.
- Backtest: **script indicators as conditions**: use any plot of a Pine Script v5/v6 compatible indicator in a strategy and optimize its inputs; the chart updates scripts live.
- Custom indicators: scripts using arrays, string functions, user-defined types and methods now run, with 28 more `ta` functions.
- Visualization: script indicators now draw their labels, lines, boxes, tables, fills between lines and polylines on the chart.
- Visualization: smoother panning, zooming and live updates on charts with script indicators, which now warm up on 5,000 stored bars and appear one by one as soon as each is ready.
- Visualization: a **bid / ask** chart option draws the broker's live best bid and ask on live charts (crypto exchanges, Capital.com, Alpaca, Massive, Interactive Brokers).
- Live feeds: Alpaca option charts stream live.
- Live feeds: a notification when a live feed stays down and when it is back, chart alerts say when they cannot watch, and Interactive Brokers charts no longer look live on a lost connection.
- Visualization: charts that are not live run their script indicators on the server, and reopen without recomputing them.
- Visualization: workspaces are **saved on request** (Save, Ctrl/Cmd+S, Save as, Revert) with a prompt before leaving unsaved changes; studies now follow the chart.
- Visualization: **chart templates** save a chart's studies and layout by name and apply them to one or several charts.
- Visualization: **resize indicator panes** by dragging the boundary between them, saved with the chart layout; double click resets.
- Visualization: **one toolbar and one drawing rail** shared by every chart, acting on the chart in focus, and a fullscreen button that hides the app top bar.
- Settings: **Bybit** and **Bybit EU** as keyless data connectors (spot, linear and inverse contracts, live candles) and read-only broker accounts covering options, regional sites and RSA keys.
- Settings: **KuCoin** and **KuCoin Futures** as keyless data connectors with live candles, and read-only broker accounts for spot and USDT/USDC-margined futures.
- Settings: **Gate.io** as a keyless data connector (spot and USDT perpetuals, live candles) and a read-only broker account covering classic and unified accounts.
- Settings: **MEXC** as a keyless data connector (spot and perpetuals, live candles) and a read-only broker account for spot and futures.
- Settings: **Bitfinex and Deribit** as keyless data connectors with live candles, and read-only broker accounts (Deribit: perpetuals, futures, options and spot).
- Historical Data: **Check gaps** on a dataset finds missing candles, tells them apart from holidays and quiet minutes, and fetches the ones you pick after showing the request estimate.
- Historical Data: dataset rows highlight on hover and show their exact range, span, provider and last update.
- Visualization: **alerts compare two series** (SMA 9 crossing SMA 21, price crossing an EMA), % change over N bars, relative volume or a Pine Script plot, with an optional expiry date.
- Visualization: an **All alerts** list to edit, pause, re-arm, delete or open any alert in its chart, with filters and Pause all; alert notifications now open the chart.
- Backtest: **relative volume** (the bar's volume against the average of the N bars before) as a condition operand.

### Changed
- Visualization: the indicator sheet has Inputs and Style tabs, with a line style choice.
- Visualization: cleaner hover tooltip, countdown under the last price at the same width, and a workspace menu with add, rename and delete.
- Visualization: a non-blocking notice while the latest candles load, and the settings menu closes on an outside click.
- Historical data: timeframe filters are sorted from shortest to longest.
- Security: dependency updates for the web build and the HTTPS image.
- Data connectors: every connector and broker account runs on the **new connector framework**, and their refusals say whether to retry.
- Data connectors: OANDA and FOREX.com downloads also store bid and ask bars, and TradeStation serves 1h bars.

### Fixed
- Agent: finishing a response no longer jumps away from older messages; the chat preserves the reading position unless it is following the latest message.
- Historical data: Alpha Vantage intraday bars are stamped in UTC instead of New York time, and bars already stored are corrected on upgrade.
- Brokers: TradeStation short sales import as sells.
- Brokers: a Capital.com account whose Account ID is the preferred account no longer fails to connect.
- Live feeds: a dropped, refused or silent connection reconnects on its own and catches up missed bars (charts, paper trading, news).
- Live feeds: a chart opened or reconnected mid-candle keeps that candle's real open, range and volume, and recorded intraday candles are corrected from the provider once final.
- Historical data: a download or a file import no longer stores a candle cut short by the end of the range or still in progress.
- Live feeds: the lag shown on crypto exchange charts is exact to the millisecond instead of sometimes reading up to one second too high.
- Visualization: charts hidden by a smaller grid stop streaming, without interrupting the others.
- Visualization: the baseline chart type no longer breaks the chart until reload.
- Visualization: background alerts are judged seconds after each candle closes; a 1m alert was read only every other minute.

## [0.0.16] - 2026-10-10

### Added
- Agent: a **Memory** toggle in each chat turns long-term memory off for that conversation.
- MCP: **OAuth sign-in** for clients that cannot take a fixed token (claude.ai, ChatGPT connectors), approved on a consent page with per-module permissions.
- Demo: Fundamentals opens with **illustrative data** on every page (macro, companies, ETFs, calendar, alternative data), clearly marked as made up.
- Mobile: **responsive layout, phase 1**: navigation, module pages, dashboard widgets and charts adapt to phone screens.
- Backtest: a small notice on the results when the run had no spread or no fees set.
- Security: **social sign-in** with Google, Microsoft, GitHub or any OpenID Connect provider, locked to one linked account, with recovery codes and the option to turn password sign-in off.
- Agent: **per-chat limits** on output tokens or dollars, with a default for new chats, a budget bar beside the message box, and a notice offering to raise the limit or start a new chat once it's reached.
- Voice: **push-to-talk** commands and dictation, chained voice commands confirmed before they run, and a choice of speech engine (browser, self-hosted Whisper, OpenAI, Groq).
- Settings: **Clean** in Manage data deletes a module's records older than a chosen number of days, weeks, months or years (mail, news, calendar, notifications, runs, chats and more).

### Changed
- Backtest: new strategies start with no fees instead of 0.1%.
- Agent: a chat keeps the provider and model it started with, and picking a provider in a chat selects that provider's default model.
- Agent: the API key is optional for a provider, so local servers like Ollama work without one.
- Agent: the write confirmation shows how to skip it next time (approve writes automatically on the persona).

### Fixed
- Security: actions that ask for your password again (new MCP token, network mode, credential export, vault, turning 2FA off) work again over HTTPS.

## [0.0.15] - 2026-10-04

### Added
- Editor and Backtest: opt-in **versioning** of files and saved strategies: save a dated version with a note, preview, restore or delete it; a deleted strategy can keep its history.
- MCP: agents can save, list, read, restore and delete versions of files and strategies, and switch versioning per item.
- Fundamentals: new module with **macro series** from 13 public sources, company statements, filings, insider trades and transcripts, ETFs, a market calendar and alternative data, stored locally.
- Fundamentals: stored series and followed companies refresh on their own and notify new data; companies, series and filings show in search and are readable by agents.
- Fundamentals: choose the order providers are tried in for each dataset, and pages skip a provider that just refused or ran out of quota until you press Refresh.
- Dashboard: **Fundamentals widgets** for a macro series or board, a company snapshot, a statement line over time, your companies, latest filings, upcoming earnings and valuation against stored peers.
- Dashboard: Quant **volatility and regime widgets**, dataset and compatible-basket selection, and configurable seasonality metrics and VaR confidence.
- Agent: **dump a conversation to the Editor** in a folder you pick, with dates, persona and model, reasoning in a quote, and optional tool calls and tokens.
- Agent: the **Financial Analyst** studies companies from Fundamentals (statements, ratios, filings, transcripts); reset the persona to pick it up.
- Data broker: SEC EDGAR, FRED, FMP, Finnhub, Quiver Quant, USPTO, LDA.gov and the public statistics providers are connectors; Alpha Vantage, EODHD and Massive keys serve fundamentals too.
- Backtest: **limit orders** for entries and signal exits, a lower-timeframe check that downloads only the candles it needs, a separate maker fee, and **bid/ask fills** fetched for the bars that fill.
- Paper trading: stops, targets and limit orders **trigger on the live price** with an immediate alert, signals stay on closed candles, and a provider without a live price is named.
- Historical Data: bid/ask from Capital.com, OANDA, Interactive Brokers, FOREX.com, Alpaca and Massive, with a table of what each provider offers in the docs.
- Paper trading: each session has a **full-page dashboard** with a back button, live prices, its book, a chart of entries and exits on the strategy timeframe, and the backtest analytics including 3D scatter and streaks.
- Backtest: **holes in the data** are flagged, trades crossing one are excluded from the results, and one click downloads the missing candles.
- Backtest: a **price tick** that rounds every fill and level against the trader, negative fees for rebates, and trading-window time zones that follow daylight saving.
- Fundamentals: Company, ETF and Alternative data share a **symbol picker** with favourites and recently opened symbols.

### Changed
- Editor and Backtest: versions open in a searchable window, the current one is marked, a restore is saved as a new version, and notes are capped at 500 characters.
- MCP: agents can read and switch the editor and strategy versioning settings.
- MCP: module listings no longer carry body schemas; an agent fetches one endpoint's schema when it needs it, so large modules like Backtest are listed in full.
- Dashboard: Fundamentals and Quant widgets show units, reporting basis, history coverage and sources, refresh stored results every five minutes, and adapt tables and accessible heatmaps to narrow cards.
- Backtest: grid and DCA strategies only show the filters, sizing and advanced settings they actually use.
- Backtest: stops and targets are tested on the candle a trade opens in, the take profit fills as a limit, and a trade closed mid-candle no longer reopens at that candle's open.
- Backtest: a **neutral grid** trades long below its centre line and short above it, entry fees leave the cash at the fill, and Kelly sizing resumes once the edge comes back.
- Backtest: settings with no meaning and malformed candles are refused before the run, naming the problem.

### Fixed
- Dashboard: Quant correlation reads the nested API matrix; statement growth matches the same fiscal period a year earlier, and negative statement bars extend below zero.
- Dashboard: earnings can use a stored calendar when a company snapshot is missing; statement labels keep their space, and phone layouts stack cards without horizontal page overflow.
- Search: content titles no longer fail to load when Resources is not among the searched modules.
- Backtest: ATR stops and targets use the ATR of the last closed candle, no longer that of the entry candle, whose range is not yet known when the trade opens.
- Backtest: position size, pyramid adds, margin, exposure caps and circuit breakers use what is known at the candle's open, no longer its close.
- Backtest: no new trade opens at the open of a candle whose close an expired exit limit closed the previous trade at.
- Paper trading: an entry is **alerted at the close that signals it**, a market order then fills on the first live price and a limit order is confirmed when it fills.
- Backtest: a lower-timeframe or bid/ask download stops after the provider refuses it, and the run's warnings say so.
- Backtest: **no decision fills at the close that made it**: exit when the entry stops holding, and an expired exit limit, now leave at the next open.
- Backtest: a pyramid add fills before the candle's stop, which then closes the larger position and is tested on the stop the add moved.
- Backtest: portfolio slots, sizing and caps see only what is known at each asset's own open, sessions opening later included.
- Backtest: grid buys only below the last close, fills gaps at the open, stops at its level, flattens at the window's open, and without bounds uses the range known so far.
- Backtest: the optimizer and sweep rank on the in-sample part under a split, and the multiple-testing haircut uses the data's real candles per year.
- Backtest: stocks run on split and dividend adjusted prices where the provider has them, with a **raw prices** option and a warning otherwise.
- Backtest: take profit can require the price to **trade through** it, the trailing stop and MAE/MFE ignore prices from before a fill or after an exit, and tick bid/ask uses the opening spread.
- Paper trading: announced orders are sized on marked equity and carry their **amount**, for brokers that take notional orders, shown in the dashboard's orders tab.
- Backtest: a grid candle that opens past one circuit stop exits at that stop, even when it also reaches the other.
- Backtest: DCA MAE and MFE are positive amounts that count the contract multiplier once.
- Backtest: the warm-up counts indicators left at their default length and MACD's signal line.
- Backtest: an EMA or WMA chained on a series with gaps leaves the gaps empty instead of averaging zeros.
- Backtest: Kelly sizing no longer counts a breakeven trade as a win.
- Backtest: **reverse side** mirrors oscillator levels (RSI below 30 becomes above 70) and keeps ADX, ATR and volume filters unchanged.
- Historical Data: the candle still forming is never stored, and a last candle saved before its period ended is read again on the next update.
- Backtest and paper trading: no position is carried across missing data, and entries wait for indicators to warm up again after a hole.
- Paper trading: a session downloads the candles missing after a pause by itself, and holes the provider has no data for are reported, not retried.
- Backtest: the Markdown export no longer multiplies circuit-breaker thresholds by 100.

## [0.0.14] - 2026-09-30

### Added
- Journal: **schedule a broker sync** to pull new trades on its own, at the interval you pick.
- Journal: an instrument whose fills do not say open or close is held back and asked in the pending tasks; the rest of the sync still lands.
- Portfolio: **schedule the broker alignment**, lines needing an answer wait in the import window instead of being guessed.
- Journal, Portfolio, Tax: a long broker import shows its estimated time and can **run in the background**, with a notification when it is done.
- Automator: **Jobs** tab listing every background job (broker syncs, mailbox polls, FX rates, managers' portfolios), with pause, interval, run now, delete and run log.
- Backtest: **3D scatter and streaks** for signal runs, per asset or the whole basket, and a 3D timeline of a DCA plan's entries, green in profit, red in loss.
- Backtest: pick a **backtest period** that applies to every selected dataset, each clipped to the candles it holds.
- Charts: alerts can watch **live while the app is open**, on closed bars in the background, or live in the background, and fire once, every time, per bar close, per cooldown or per session.
- Settings: **Exchanges** with hours, lunch breaks, holidays and half days for 24 venues, refreshed weekly, and the one picked in Defaults shown open or closed in the top bar.
- Settings: **Market sessions** (Asia/Pacific, Europe, US by default) that you can edit or extend.
- Journal: **3D scatter** tab, three axes of your choice (market session included) in a box you turn by hand.
- Backtest: optimizer **3D cloud** tab, three axes among metrics and swept parameters, with a min/max slider per axis to filter the variants.
- Journal: **Streaks 3D** tab, every winning and losing streak drawn trade by trade from zero, with the average path and front, top and free views.
- Tax: **country regimes** for France, Germany, UK, US, Switzerland, Spain, Italy and Australia, checked against official sources, with the review date beside each country.
- Tax: broker reads match each sale to its purchases by the country's rule (average price, FIFO, UK share matching), convert each leg at its own date, and defer washed losses.
- Tax: French crypto follows the global-portfolio formula, with the 305 EUR threshold.
- Tax: **loss carry-forward registry** per profile, with each loss's expiry year.
- Journal: **SL/TP signals** replay closed trades under 30 simple stops, targets and indicator exits, ranked against your real exits with curves, per-trade charts and an agent to tune them.
- Install: `--admin-user` on the server installer sets the OpenTraderWorld login name.
- Docs: **When it breaks**, the common hosting problems with what to do, and a link to the guided install on the website.
- AI agents: **Call log** in Settings, every agent tool call of the last 30 days with its failure rate, causes and slowest endpoints.
- AI agents: writes accept a retry key, so a write retried after a timeout is not booked twice.
- Agent: each chat in the sidebar shows the time since its last message, and the thread shows the time at the start of each stretch of messages.
- Quant: **Statistics** tab with distribution, autocorrelation, Hurst, variance ratio, unit-root tests and Sharpe significance for one dataset.
- Quant: **Volatility** (range estimators, cones, GARCH forecast), **Regimes** (hidden Markov states) and **Events** (returns after a condition) tabs.
- Quant: **Pairs** (cointegration, spread, lead-lag), **Basket** (PCA, clusters, HRP, relative strength, stress) and **Factor regression** tabs.
- Quant: **Trades** and **Compare** tabs for saved backtests: R-multiples, SQN, MAE/MFE, run correlation, deflated Sharpe and overfitting probability.
- Quant: **volatility targeting** and **risk of ruin** sizing tools.
- Quant: **calculators** for option prices and greeks, implied volatility, futures basis, compounding and Sharpe significance.
- Quant: **Derivatives** tabs on an Interactive Brokers or Massive connector: futures curve and roll, implied-volatility surface, implied against realized volatility.
- Backtest: the parameter sweep reports the probability of backtest overfitting.
- AI agents: every new Quant tool is available to agents.

### Changed
- Journal: market data downloads fewer windows, keeps more history before each entry and shows its progress along the Market data block.
- Tax: a profile on a country regime uses that country's rates; only the custom regime keeps hand-typed rates.
- Portfolio: cost basis is kept in the portfolio's currency at each operation's rate.
- Backtest: the optimizer window, ranking table and results page are reorganized and easier to read.
- AI agents: a wrong field, query param or path is answered with every problem at once and the closest valid endpoints, and a param the server would ignore is flagged instead of silently dropped.
- Journal: the trades list pages with limit and offset and reports the total.
- Quant: tabs are grouped into Asset, Multi-asset, Strategy, Sizing, Calculators and Derivatives.
- Connectors: a connector can be granted to Quant.

### Fixed
- Journal: trades shorter than two candles are measured, and a download no longer stays stuck on a window a few seconds wide.
- Backtest: the sweep's deflated Sharpe used a wrong variance and ignored skew and kurtosis.

### Security
- Journal: trade images must be files uploaded to the app, never an outside URL.
- Demo: importing agent skills from the shelf is refused.
- Market data: a Massive error message no longer shows the API key.

## [0.0.13] - 2026-09-24

### Added
- Install: server-setup.sh script

## [0.0.12] - 2026-09-22

### Added
- Settings: **Security** section, turn on two-factor authentication and see every browser signed in to your account.
- Settings: close one signed-in browser or every other one at once, with the source and last use of each shown.
- Settings: a sign-in from a source this account has never used raises a notification on your enabled channels.
- Settings: set how long a single request may run before it is stopped, from the Security section, applied immediately.
- Recovery: `disable-totp <username>` on the host removes a second factor when the authenticator is gone.
- Webhooks: point an endpoint at an **Automator workflow**, so an incoming alert starts a run and its payload is readable in every block.
- Settings: **Backup & restore** in one section, Full or Partial, each with a backup and a restore side.
- Settings: **partial backup**, download the modules you pick as one zip and load it back on any instance, adding what is missing or replacing a module outright.
- Settings: the partial backup counts what you are taking, per module and per table, and a file you load is read first to show what it holds against what is here.
- Settings: the Backup & restore section shows when the last automatic backup ran, where it went and whether it was restore-tested, on instances that run one.
- Chart: **delete a custom indicator** from the library, with a confirmation step; charts already plotting it keep theirs.
- Settings: **Brokers**, read-only broker accounts (Alpaca, Binance, Coinbase, Kraken, Interactive Brokers Flex) shared by the modules you allow.
- Journal: **import your trades straight from a broker** for a period you pick, previewed first, and re-running it never files the same position twice.
- Chart: **sync a broker account** to draw its positions and working orders on the chart of the matching instrument.
- Portfolio: **import what a broker account holds**, line by line, with the difference against your ledger shown before anything is written.
- Taxes: **read a tax year straight from a broker**, realized gains split across the capital, derivative and crypto lines of the form.
- AI agent: **memory search**, the assistant can find a stored fact that is no longer spelled out in its context.
- AI agent: the token badge counts what was served from the provider's cache, apart from what was paid in full.
- AI agent: **usage breakdown**, tokens and provider requests by day or hour, persona, model and provider, from a stat icon in the chat header.
- AI agent: **global overhaul**, answers that are faster, steadier and cost fewer tokens for the same work.
- Settings: **external control**, drive the app from Telegram, Slack or Discord under the module rights of the token you pick, answering only paired senders and confirming every write in the chat.
- Symbols: a **(?) next to every symbol box** shows how each provider spells the same instrument, from BTCUSDT to MNQU6.
- Historical data: **import your own OHLCV file** (CSV, TSV, TXT, JSON, Parquet), columns detected and shown in a preview, filed under a name, source and tags you pick.
- Historical data: **export a dataset as Parquet** as well as CSV, typed, far smaller, and it carries the instrument it holds so re-importing it fills the form itself.
- Journal, Portfolio: trade-book and ledger imports read **Parquet** files too, through the same column detection.
- Settings: an external control binding sets the provider and model new chats start on, and `/provider` and `/model` switch one chat from the chat itself.
- Settings: **Security** shows a QR code to scan when turning on two-factor authentication, beside the link and the secret it already gave you.
- Portfolio: **take the price from the exchange** on a broker line with no cost basis, with the market that answered named on the line.
- Journal: **name the market** a currency like USDT or USDC is priced on, a connector plus the pair as that venue writes it, so it gets a rate instead of staying pending.
- Settings: **four broker accounts more**, OANDA, Bitget, OKX and Binance USDⓈ-M Futures, read-only like the others.
- Settings: **four data connectors more**, OANDA for FX and CFDs, Bitget, OKX and Binance USDⓈ-M Futures for crypto, the three crypto ones live too.
- Settings: **TradeStation and FOREX.com**, as broker accounts and as data connectors, shares, futures, options and FX on your own read scopes.
- Settings: **Capital.com and NinjaTrader** as broker accounts, and Capital.com as a data connector with live candles.
- Journal, Taxes: a broker whose archive stops short now refuses a period it cannot answer and names the date to start from, instead of returning nothing.

### Changed
- Account: passwords must be at least 12 characters and are refused if they appear in public breach lists, decorations and letter swaps included.
- Settings: minting an access token, changing the network mode, writing a vault item or exporting credentials now asks for your password again.
- Setup: on an instance reachable from the network, the first account needs the setup token printed in the server log at startup.
- Health: the public status endpoint no longer names the version to a caller who is not signed in.
- Webhooks: a flood of bad tokens now only silences the sender it came from instead of pausing every delivery.
- Backtests, sweeps and Monte-Carlo runs queue on a share of the cores instead of all running at once.
- Install: `setup.sh` writes `.env` readable only by you and blanks the bootstrap admin password once the account exists.
- Images: published images carry a build-provenance attestation and an SBOM, so a pull can be checked against this repository.
- AI agent: **prompt caching**, a long conversation and a tool run now re-read most of their prompt instead of paying for it every turn.
- AI agent: compacting a long conversation can run on a cheaper model, set `summary_model` in the agent's advanced params.
- AI agent: a throttled provider is retried instead of ending the message, so a rate limit mid-run no longer discards the work already done.
- AI agent: an oversized tool result is capped before it reaches the model, with a note saying how to ask for a narrower slice.
- Health: the status endpoint now reports how long the service has been up, so an update can be confirmed to have actually restarted it.
- Settings: an external control pairing code now lasts an hour instead of ten minutes, and the window is yours to set, from 5 minutes to a day.
- Historical data: importing or downloading bars writes them in blocks instead of one at a time, so a large file lands in a fraction of the time.
- Historical data: the export and file-picker buttons say they are working instead of looking dead while a big file is read or built.
- Sidebar: the Tools section lists every installed tool instead of stopping at eight, and scrolls when the window is too short for them.
- Settings, docs: the update instructions are now given per install type, an image install refreshes `deploy/` from the release before pulling, a source build resets the checkout and rebuilds.


### Fixed
- Journal: a broker fee billed in a currency with no rate is no longer subtracted as if it were the trade's, it is left out, flagged on the trade, and raised as an FX task.
- Journal: an Interactive Brokers cancellation now undoes the fill it names instead of doubling the position.

### Security
- Pages ship a strict Content-Security-Policy, so injected markup cannot run script against your session.
- Over HTTPS the session cookie is `__Host-` prefixed, which no other host on the domain can overwrite.
- A request that changes something is refused when the browser reports it came from another site.
- On a network-facing instance a session also expires after a day of inactivity, on top of the existing one-week limit.
- Repeated failed sign-ins slow down the source they come from, without ever locking the owner out.
- Notifications: a **security** feed on your channels for a run of failed sign-ins, a token minted or used for the first time, a new chat sender paired, a change of network exposure and an export that carried your credentials.
- Settings: those same events are written to the log under `security`, so searching that word shows what was attempted against the instance.
- Deploy: every container now runs with no Linux capabilities beyond the one it needs, no privilege escalation, a memory and process ceiling, and a read-only filesystem wherever it writes only to its volumes.
- Deploy: the DNS forwarder image is pinned to a digest instead of a floating `latest` tag.

### Removed
- Settings: the About links no longer carry a referral tag, they go straight to the site with nothing appended.

## [0.0.11] - 2026-09-12

### Added
- Dashboard: widgets are **interactive**, hovering a chart reads out the value under the cursor, and a day on the calendar grid lists its events.
- Dashboard: widget charts gain a selectable time range and a heatmap that switches scale.
- Dashboard: each widget refreshes on its own timer, chosen in its settings, pausing while the tab is hidden and catching up on return.
- Dashboard: a **privacy eye** in the top bar blurs every widget body at once, keeping titles and tools readable.
- Dashboard: the widget picker browses by module, with a count and a short description per widget.
- Dashboard: the header carries a favourites bar of up to four dashboards, next to Customize, Add widget, the clock and the calendar shortcut.
- Dashboard: the navigation rail splits into Workspace, your dashboards, and Tools, every installed module, both arranged by drag and drop.
- Dashboard: row height is adjustable while customizing, and a Cancel button drops the changes made since you started.

### Changed
- Dashboard: the favourites bar can now keep up to five dashboards at hand.
- Interface: the whole app is redesigned, new colors, spacing and controls, with every module page redrawn on the same visual language.
- Dashboard: a page is now one free flow of tiles over 12 columns, dropping a tile pushes the others along instead of editing fixed rows.
- Dashboard: a widget body adapts to the width of its card, so a two-part block stacks in a narrow tile and splits in two columns in a wide one.
- Dashboard: a widget is titled by what it shows, not by the module it comes from.
- Modules: the module switcher lists modules alphabetically, dashboard first.
- Interface: a new OpenTraderWorld logo in the rail and the browser tab, drawn for the active theme.
- Appearance: the default accent colour is now sage green.
- Dashboard: the Portfolio preset is rearranged so its cards read in the order you use them.

### Fixed
- AI agent: a longer default reply budget, so a long answer or a big tool call is no longer cut off halfway.
- Backtest: the strategy presets all run as they ship, and a recorded run can be read back with the exact settings it used.
- Backtest: a grid strategy is now locked to a single dataset, instead of accepting a basket and quietly running the first one.
- Routines: dates follow the app language instead of the browser's.
- Demo: the demo data now fills every card of the five preset dashboards, portfolio positions, cash ledger, journal positions, backtest runs and headlines included.

## [0.0.10] - 2026-09-07

### Added
- Visualization: a chart setting shows the time left before the running candle closes, under the last price on the scale.
- FinanceDatabase: the instrument catalog says which snapshot it holds and offers a one-click update when a newer one is published, favorites following to the new rows.
- Visualization: the chart page is now a **workspace**: pick any grid from 1x1 to 3x4, drag the splitters, and every pane keeps its own instrument, timeframe, studies and drawings.
- Visualization: an instrument list beside the chart, from your Watchlists and from lists the chart owns, with drag onto a pane and one action to turn a chart list into a real watchlist.
- Visualization: several charts now share one live connection, so two panes on the same account stream at once instead of the second one waiting for a seat.
- Visualization: daily and weekly charts now stream live on Alpaca, Massive and Interactive Brokers, the candle picking up where the session's own bar left off.
- Connectors: Alpaca gains the **weekly** candle, downloaded and streamed, its week running Monday to Friday's close on equities and Monday to Sunday on crypto.
- Connectors: Massive **futures** now stream live, alongside the stocks, options, crypto, forex and index feeds.
- Connectors: Interactive Brokers now streams **trade by trade**, so the price moves on each print instead of stepping every five seconds (options and non-CME indices keep the five-second bars).
- Visualization: indicators and drawings stay with an instrument even when you never saved it, and reopen the same way in any pane.
- Visualization: **price and indicator alerts** watched by the server on closed bars, so they fire with the browser closed and land in your notifications and your channels.
- Visualization: compare instruments on one chart, rebased to percent or shown as a ratio, and link panes by colour so they follow the same symbol and the same instant.
- Visualization: an objects list with hide, lock and reorder, day separators, the previous close, and a one-click PNG of the chart.
- Visualization: drawings can be copied between charts (Ctrl+C, Ctrl+V, Ctrl+D), snap onto the objects already there with a guide line, and their style can be saved as a named template.
- Visualization: save a chart as a page, the picture plus the candles it was drawn from, in one file that opens in any browser offline.
- Journal: a new **Behavior** tab shows where the profit really comes from, what a losing streak does to your size and your pace, how the day decays, and names the habits that cost money.
- Journal: load the candles of your own tickers and a **Market data** tab answers what the trade log cannot: MAE and MFE, whether the stops sit in the noise, how much of each move you kept, and in which volatility the strategy works.
- Journal: an **Open risk** tab shows what is still on the line right now, where that risk is concentrated, and whether your open positions are separate bets or the same bet several times over.
- Backtest: a **trailing stop** follows a signals trade, in % or in points, with an optional profit threshold before it starts and a step to breakeven.
- Connectors: **Interactive Brokers** joins the data broker, reading stocks, ETFs, crypto, forex and indices from your own IB Gateway or TWS, with a Test connection button that names what to fix.
- Visualization: **live charts for Alpaca, Massive and Interactive Brokers**, on every intraday timeframe, with the account to stream on picked in the live control when a provider holds several.
- Visualization: a live feed that cannot run now says why, in the provider's own words, and stops instead of reconnecting forever behind a dot that never turns green.
- Visualization: going live no longer skips a candle, the bars that closed between the downloaded window and the first live tick are sent when the stream opens.
- Journal: futures trades can now be enriched with market data, once you name the contract your source uses (MNQU6, MNQ.202609@CME) instead of the root you log.
- Journal: put your **trading routines** on a book's PnL Calendar over a period, and each traded day shows a dot, green when they were all done, red when none were, amber in between.
- Backtest: **paper trading** runs a strategy forward on your schedule, alerts your notification channels with a message you write yourself, and holds what it could not deliver until you come back.
- Backtest: a paper session downloads the candle it waits for and runs on the candle grid, every minute at :00, every 15 minutes at :00/:15/:30/:45, hourly on the hour.
- Backtest: a paper session words its entry, its exit and its grouped summary separately, each with its own variables and its own preview.
- Backtest: saving a strategy asks whether to overwrite the one it came from or keep it as a new one.
- Portfolio: the ledger now holds **deposits, withdrawals, dividends, interest, coupons, fees and taxes**, so a portfolio finally has a cash balance, an income figure and a real net worth.
- Portfolio: a new **Analysis** tab answers performance and risk in one place: return and IRR per period, annualized, volatility, max drawdown, Sharpe, Sortino, Calmar, best and worst month, and how long each drawdown took to fill.
- Portfolio: pick a **benchmark** and the page says what it would have returned at your own volatility, next to what you actually made.
- Portfolio: set a **target allocation** per asset class with a tolerance band, and the page shows the drift and the trades that would close it.
- Portfolio: **stress testing** replays 2008, 2020, 2022 or shocks the book with S&P, Nasdaq, rates, EUR/USD, oil and credit, and always says what share of your positions the number actually covers.
- Portfolio: rebuild your **daily history** from the ledger and stored candles, so a portfolio kept for years gets its whole curve instead of starting the day you switched the job on.
- Portfolio: a broker statement's dividend, deposit and fee lines now import as what they are, instead of being listed as errors.
- MyWealth: **liabilities**. A mortgage or a loan is an asset that is owed, so the headline is at last a net worth and not a total of what you own.
- MyWealth: a portfolio is now **linked, not copied**: its value is read live from the tracker every time, and never goes stale.
- MyWealth: tell an asset how often it should be revalued, and the page says when a valuation has aged past it.
- Agent: a new **stress-and-diversify** skill reads a stress result, names the factor carrying the loss, and proposes diversification with the coverage stated first.
- AI agents: an agent now reaches the chart's workspaces, lists and alerts, paper trading, the journal's market data and routines, a portfolio's cash rows and history, and a connector test.
- Demo: the public sandbox now opens the **chart workspace** and the automator, with a seeded grid, rail lists, alerts and a workflow with its run history.

### Fixed
- Visualization: quick backtest sizes now read everywhere in the unit you clicked in, contracts or currency, abbreviated past a thousand.
- Visualization: a daily or weekly live candle is now corrected by the provider once the period closes, even when a gap was being filled at the same moment.
- Visualization: a daily chart no longer fires a pointless download every weekend and holiday, and no longer re-reads the session's start for each pane you open.
- Visualization: a market that just changed to or from summer time re-aligns its live daily candle on the next close instead of at the next reconnection.
- Visualization: the live button now says what is actually forming (trades, minute bars, the candle itself), and a refused feed shows what the provider's plan requires.
- Visualization: a quick-backtest marker sized in contracts now reads the number of contracts you clicked instead of the units they bought.
- Visualization: an image of the chart now carries the drawings on it, instead of the candles alone.
- Visualization: a comparison series now lines up with the chart period by period, so two markets that stamp the same day differently no longer sit a day apart.
- Visualization: a chart on Interactive Brokers says when the account's market data lines are nearly all taken, instead of a later pane going quiet with no reason given.
- Visualization: a pane too narrow for its own furniture now drops what it cannot fit rather than writing the readout over its candles, and its close, link and maximize buttons always stay in reach.
- Backtest: the Optimizer now offers a DCA plan its own parameters (contribution, tranche amounts, sell objectives, rule conditions) instead of sizing knobs the mode ignores.
- Backtest: with a single dataset selected, the Data step now shows its period, its candle count and the row grain, as a portfolio run already did.
- Backtest: clicking a variable inserts it in the message being written again, and the preview renders the whole message, marking only the values it cannot read yet.
- Portfolio: the Analysis tab no longer reports a portfolio it could not price as worth nothing, which pinned the return at -100% and wrecked every measure built on the curve.
- Portfolio: the benchmark comparison now measures your book over the index's own sessions, instead of dropping every Monday and understating your return by a third.
- Portfolio: a period longer than your history is marked as not covered rather than reported as if it were, and a return under two months no longer gets annualized.
- Portfolio: refreshing a portfolio now downloads the missing candles and rebuilds the daily curve behind it, and says so when a broker cannot serve one of your instruments.

## [0.0.9] - 2026-08-30

### Added
- Editor: a **video** block turns a YouTube or Vimeo link into a thumbnail and a link, so a document carries a video without carrying its weight.
- **Automator**: a new module stacks tasks in a grid of steps, calls your own API or any URL, asks an AI step, and notifies, by hand or on a schedule.
- Automator: every run is kept step by step, with the request and the answer of each block, so a workflow that failed at 3 a.m. says which block failed and why.
- Automator: schedules run in a real timezone, skip an occurrence while the previous run is still going, and can catch up once after the app was down.
- Automator: the editor **saves on its own** and every edit can be undone or redone, with a block still being filled in kept as a draft the schedules ignore.
- Automator: a workflow card runs the workflow now, and pauses or resumes its schedule from the list.
- Automator: a test run executes the reads and reports what each write would have done, including the blocks that were only waiting on one, and a block that failed hands its error message to the next one.
- Backtest: a **DCA** mode simulates a savings plan, a weighted basket bought with the starting capital, a recurring contribution and conditional tranches, sold on an objective or a signal, with fees.
- Backtest: conditions can read a market metric (drop from the high, rise from the low, change over N bars) and, in a DCA plan, the live position (P&L %, drift since the last buy, weight, cash).
- Backtest: the exported report carries the whole result, the PDF with every chart and the Markdown with the figures alone, per side and per asset, and never a trade list.
- Backtest: an exported report is named after the strategy it came from and the moment it was exported.
- Backtest: an Optimizer varies the parameters of a finished run (indicator lengths, thresholds, stops, sizing, weekdays to exclude), tells you how many variants that is and how long it will take before starting, then ranks them on a page you can stop and read at any point.
- Backtest: a Filters step decides when the strategy may open, by session hours on a chosen UTC offset, by weekday and by date, holding or closing the position once the window shuts.
- Backtest: a finished run takes the whole screen and gains the chart's analysis views: statistics by side, streaks, return distribution, and a cumulative P&L curve with its drawdown.
- Backtest: a grid can follow a moving line (SMA, EMA, DEMA, TEMA, WMA, HMA, VWAP) with its band set in % or in ATR, and can be rebuilt at every candle close.
- Backtest: clicking a trade in the list frames it on the chart, on its own asset for a portfolio run, zoomed from the trade's own length.
- Backtest: the per-asset tab reads one asset at a time, with the same statistics and P&L curve the whole run gets.
- Backtest: the Run button fills as the run works, paced by what past runs of that kind measured on your machine.
- Backtest: a DCA run reads what the plan holds at the last bar (units, average cost, value, weight against target, invested, realized, unrealized, fees) with its fill list, in the result tabs and in the report.
- Dashboard: the **Modules** page is a real page you can edit, seeded by section (markets and data, trading, wealth, information, productivity, automation and AI) and reconciled with the modules you install.
- Journal: an **Analytics** view reads the whole book, statistics by side, streaks, profit distribution, equity curve and drawdown, discipline tags and the planned stop, with the breakdown filtered by ticker.
- Chart: TEMA, rolling VWAP, SuperTrend and ADX join the indicator catalog.
- Historical data: one download form queues several series at once, several timeframes and several symbols separated by commas, and prices the batch in provider requests before it starts.
- Historical data: a download that runs into the provider's quota or rate limit waits and resumes on its own, showing why and for how long, and any job or whole batch can be cancelled.
- AI agents: an access token can be given an expiry date, after which it stops working everywhere.
- AI agents: an agent can list the parameters a strategy can sweep and drive the Optimizer, and reaches the journal CSV export and cross-module search.
- AI agents: an agent granted the Automator can read your workflows, create one, write its graph and test it, while putting one into service stays yours: its graph arrives as a draft you adopt from the editor, and it can neither attach a workflow's access token nor run one.
- Notifications: a channel destination (a Telegram chat id, for instance) can be plugged from the vault instead of being typed in clear.
- RemindMe: "Clear all" empties the whole notification history, behind a confirmation since it cannot be undone.
- Vault: a secret value is revealed while you hold the eye down, and hidden again as soon as you let go.

### Changed
- Quant: a basket mixing a 24/7 market with an exchange one is measured weekly, and annualization counts the periods the data really holds instead of assuming 252 a year.
- Backtest: a run is drawn on the full visualization chart with the strategy's indicators, over a split you drag between chart and statistics, either side closing fully with a banner to reopen it.
- Backtest: the dock of past runs is dragged to any height from its top edge and clicked shut, and the two exports of a finished run sit under one Reports menu.
- Backtest: a single full-screen builder, five steps to walk or jump between, runnable as soon as a dataset is picked, above a dock of past runs and saved strategies searched by name or tag.
- Backtest: a DCA run tracks the proceeds taken out when it sells, separates realized from unrealized in the trade table, and compares against the same money in deployed at the start rather than a buy and hold.
- Backtest: a denser strategy form, with one density control for the whole builder.
- Journal: the trades list shows entry and exit times, and the quantity of an advanced trade sums its entry legs instead of showing an average price.
- UI: a modal keeps its title and its buttons in view while its content scrolls.
- AI agents: an agent runs a backtest or a risk calculation without asking you to approve it, since it stores nothing; only real writes still prompt.
- AI agents: mail and feed articles now reach an agent labelled as outside content, so instructions hidden inside them are ignored.

### Fixed
- Backtest, Quant: datasets from different providers line up on the same period, so a crypto series and an equity one can be measured or backtested together instead of never meeting.
- Community docs: pictures, diagrams, highlights and checklists published on the website now arrive whole in the app instead of vanishing when the library syncs.
- Community docs: a document submitted for publication is refused when one of its images is linked from another site, so a published doc always reads offline.
- Backtest DCA: the return, Sharpe, Sortino and drawdown of a savings plan no longer count the month's contribution as a gain of the money already invested.
- Backtest: position sizing now accounts for the contract multiplier, so futures and CFD strategies sized by % of equity, risk, Kelly or equity tiers get the size they asked for.
- Backtest: a stop hit by a candle that gapped past it now fills at that candle's open instead of at the stop price, so gap losses are reported.
- Backtest: maximum exposure limits now count the position being opened, instead of allowing one full position past the cap.
- Backtest: breakeven trades no longer count as wins, so the win rate and streaks match the exported report.
- Backtest: profit factor and Sharpe no longer rank a flawless or perfectly smooth variant at the wrong end of an Optimizer ranking.
- Backtest: the out-of-sample split now credits a trade to the period it closed in, so a position held across the split is no longer reported as zero out-of-sample return.
- Backtest: the sweep verdict on a result now reflects how long the tested history is and its timeframe.
- Backtest: reloading the page comes back on the finished result, figures already there, while the trades and curves are recomputed behind a skeleton; leaving the module still clears it.
- Journal: Sharpe in the breakdown is now the annualized daily figure shown in analytics, with Sortino beside it, so the two screens agree.
- Notifications: a long line in a notification (a list of tickers) now wraps inside its card instead of running past it.
- Search: the results panel no longer closes itself while you are still searching, whether you toggle deep search, type after dismissing it once, or click its clear button.
- UI: a field inside a bordered box (a tag box, a combo, an input group) no longer draws a second frame inside the first.
- UI: the grab handle between the chart and the panel below it is visible instead of blending into the border.
- UI: the Appearance settings read in your own language instead of English.

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
- Journal: a Scatter tab plots one dot per trade, with the two axes and the colouring picked from the trade's own values (PnL, R, holding time, size, hour, price), a trend line and its correlation.
- Journal: analytics distributions gain a profit-per-trade histogram, with the bucket width picked on the chart (automatic, or 1 to 1000).
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

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.17...v0.1.0
[0.0.17]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.16...v0.0.17
[0.0.16]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.15...v0.0.16
[0.0.15]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.14...v0.0.15
[0.0.14]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.13...v0.0.14
[0.0.13]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.12...v0.0.13
[0.0.12]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.11...v0.0.12
[0.0.11]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.10...v0.0.11
[0.0.10]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.9...v0.0.10
[0.0.9]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.8...v0.0.9
[0.0.8]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
