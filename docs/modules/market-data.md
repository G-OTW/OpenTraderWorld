# Market data & backtesting

Four modules form a chain: **Historical Data** downloads price history into local datasets; **Backtest** and **Quant Tools** work on those datasets, and **Visualization** charts any instrument a connector serves — stored or not. All three require Historical Data to be installed, since it owns the dataset catalogue they read.

## Historical Data {#histdata}

Download OHLCV candles from external providers into datasets stored in your database. Once downloaded, the data is yours — chart it, backtest it, export it, no re-fetching.

### Providers & credentials

Providers are configured once, centrally, as **[data connectors](/config/connectors)** — a named provider account with its credentials, an optional request limit, and the modules allowed to use it. Historical Data has **no provider settings of its own**: the *Connectors* button next to the provider picker opens the same shared screen you'd find in Settings.

Some providers are **keyless** (Binance, Kraken, Coinbase, Yahoo Finance) and work immediately; others need an API key — most have free tiers. A connector missing its credentials shows *needs credentials* and is skipped until you set them.

Outbound calls are counted in **Settings → API rate** so you can watch your free-tier usage.

### Downloading

Pick provider, asset type, timeframe, ticker and date range, then **Download**. Notes:

- **Futures** use contract codes: base + month letter + year digit (`F G H J K M N Q U V X Z` = Jan…Dec), e.g. `GCJ5` for April 2025 gold.
- **Options** are built from underlying, expiry, call/put and strike.
- **Intraday limits**: providers only serve intraday granularity for a limited lookback (e.g. ~7, 60 or 730 days depending on provider). Older history is available at **1d / 1w** without limit. The form warns you before you queue an impossible range.

Downloads run as background **jobs**, chunk by chunk, with live progress. Filter jobs by status, provider, timeframe or ticker.

### Datasets

The **Datasets** tab lists everything stored: bar counts, date range, size. From here you can:

- **Fetch newer** — pull bars newer than the last one stored (top up a dataset).
- **Export** the data.
- **Delete** a dataset (drops all its bars).
- Jump straight to a **chart** of it.

### Look before you download

You don't have to queue a job to find out whether a symbol is worth storing. The chart fetches a window through a connector and **stores nothing**; when the window looks right, **save** it and the normal download job is queued for exactly that range. Saving over bars you already hold consolidates rather than duplicating, so topping up a dataset from the chart is safe.

## Historical Data Visualization {#histviz}

The chart is not bound to a dataset — it opens an **instrument**. Search a symbol, pick a timeframe, and the bars arrive whether you have downloaded them or not: the server serves what is already in your catalog and fetches only the missing edges through a [connector](/config/connectors). Nothing is written unless you ask.

### Finding an instrument

The **Data** tab is one search box over every connector the chart is allowed to use. Type `BTC` and Binance, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca and Massive answer together, each hit labelled with the connector that served it. Filter by asset type; tick or untick sources in the same panel (the tick **is** the grant — the server refuses a connector this module was never given).

With the box empty the panel lists what you charted **recently**, then what is already **stored** — both open in one click and cost nothing.

### Loading history

The chart opens on the newest **1500 bars** and puts a button at the left edge of the loaded data. Each click walks one slice further back. No provider request ever happens without a gesture from you, which is what keeps a metered key predictable; the walk stops when the provider's history runs out.

The **timeframe** dropdown offers every bar size the connector supports, not only the ones you happened to download, and switching timeframe **keeps the dates you were looking at**.

### Live

For a stream-capable connector (Binance, Kraken, Coinbase), the chart **goes live by itself** as soon as the newest bar is the one forming now: the last candle updates in place, with a connection state and the lag behind the exchange.

Live is addressed by instrument, not by dataset, and **stores nothing** — any symbol a streaming provider serves can be watched live without downloading it first. Save the instrument while it's live and the same feed starts recording closed bars into the dataset as well.

### Chart

- **Chart types** — candles, OHLC bars, line, and **Renko** (with brick size).
- **Indicators** — SMA, RSI, MACD and more, as overlays or separate panes, each with configurable source, line/fill colours and width. The **legend reads at the crosshair**: O H L C, the change, and every indicator's value at the bar under the cursor, falling back to the newest visible bar when the cursor is away.
- **Chart settings** — linear or logarithmic scale, horizontal and vertical grid lines, crosshair and its per-series value tags, hover tooltip (off by default), up/down colours, price scale on the left or the right, and a **last-price tag** pinned on the price axis, tinted like the candle that produced it.
- **Navigation is hand-driven** — drag to pan, wheel to zoom (calibrated per device, so a mouse notch and a trackpad tick move the same amount). Changing chart type keeps the current zoom.
- **Volume pane** and a single fullscreen mode.

### When data is missing

A window that comes back short always says **why**, in a notice above the chart, while keeping the bars that did arrive:

| Reason | What happened |
|---|---|
| **auth** | The connector's credential is missing or rejected. |
| **quota** | The connector hit its own [request limit](/config/connectors#request-limits). |
| **rate_limit** | The provider throttled the request. |
| **symbol** | The provider doesn't know this ticker. |
| **depth** | The provider won't serve history that far back at this timeframe. |
| **provider** | Anything else the provider returned. |

## Backtest {#backtest}

*Combine indicator signals, size with pyramiding, measure the edge.* Pick one dataset or a whole portfolio, define rules, run — no code.

### Strategy

- **Entry / exit rules** per side, built from comparisons between indicators, price and fixed values. Group rules with **AND** (all must hold) or **OR** (any one is enough).
- **Direction** — long, short, or both. Options: derive the short side as the inverse of long, and **stop & reverse** (flip position when the opposite signal fires).
- **Stop-loss / take-profit** per side — each is a checkbox you turn on or off independently (percent of average entry, or an ATR multiple). With no exit rules, exits happen via SL/TP or reversal.

### Sizing, account & costs

- Size by **percent of equity** or **fixed quantity/lots/contracts**, with **leverage** and **starting capital**. Fixed quantity **scales with leverage**, following the retail convention — a leverage of 3 on a fixed size of 1 opens 3 units.
- **Pyramiding** — allow up to N stacked entries when the entry signal re-fires; SL/TP then track the average entry price.
- **Costs** — fee (fixed or % of notional, per trade or per unit) and **spread %**, so results aren't fantasy.

### Sizing (advanced)

Beyond percent-of-equity and fixed quantity:

- **Risk per trade** — size so a stop-loss hit costs a fixed % of equity (needs a stop on the traded side).
- **Fractional Kelly** — size from the win rate and payoff of the last *N* closed trades, scaled by your chosen fraction and capped; a warm-up size is used until the window fills.
- **Equity tiers** — a table of thresholds; the highest tier whose level is ≤ current equity sets the size.

### Portfolio (multi-asset)

Add several datasets and run one strategy across all of them on a **merged clock** (all locked to the same timeframe):

- An **alignment preview** shows the merged-clock length, the overlapping window, indicator warm-up bars (including the cumulative lookback of a chained custom indicator), and per-asset missing bars — before you simulate.
- **Portfolio limits** — cap the number of open positions and total / per-asset exposure.
- A **per-asset breakdown** reports trades, net PnL, fees, win rate and exposure for each instrument.

### Grid strategy

A ladder of price levels between a lower and upper bound; each cell buys low and sells at the next level up — **long**, **short** or **neutral**. Size a fixed quantity per level or split a total budget across cells, with optional stops above/below the ladder. Results report fills, round trips and end inventory.

### Costs & execution realism

- **Slippage** — a fixed number of ticks or a percent of price, applied to every fill.
- **Funding** — a constant annual rate on open notional for perp estimates (longs pay, shorts receive).
- **Circuit breakers** — halt trading after a max daily loss (for the day) or a max drawdown (for the run).
- **Instrument profile** — tick size, lot step, minimum quantity and contract multiplier, so sizes and prices snap to a realistic contract.

### Expert mode

A full-screen builder for power users:

- **Named strategies** — save, search, duplicate and edit full strategy configurations.
- **Custom indicators** — build your own from named steps, no code. Each step either applies a built-in indicator to a **source** — a price field or the output of an earlier step — or computes a **formula** referencing earlier steps by name (`@volume / SMA(@volume)`, with `+ − × ÷`, `min`, `max`, `abs`, `clamp`). This lets you chain indicators: a Hull MA of an RSI, a MACD of an RSI, a smoothed volume ratio, and so on. Indicators that read full candles (ATR, Stochastic, ADX, VWAP…) only apply to the price, not to a derived step. The highlighted step(s) are the output. Custom indicators become operands in the rule editor alongside the built-ins.
- **Searchable indicator picker** — pick indicators from a grouped, type-to-filter list (in both the rule editor and the custom-indicator builder) instead of scrolling one long dropdown.

### Date windows and parameter sweeps (API)

Two capabilities live on the API rather than in the form — they exist for the [assistant](/modules/agent) and for anyone driving the app over [MCP](/config/ai-agents):

- **Date-windowed runs.** `from` / `to` on a run (and on the alignment preview) restrict the simulated span, which is what walk-forward validation and regime slicing need: run 2019–2021, then 2022–2024, and compare. `to` is inclusive of the whole day.
- **`POST /api/backtest/sweep`** — run a parameter grid server-side and get **every trial back**, along with the trial count. Grid paths reach into arrays (`long.entry.conditions.0.left.period`), so indicator periods are sweepable; capped at 4 axes and 64 trials.

A sweep also returns a **deflated Sharpe**: the Sharpe the best of N *worthless* strategies would be expected to reach, given how much these particular trials varied. Compare the winner against that bar, not against zero — on real daily bars, the best of eight moving-average crossovers scoring 0.59 against a selection bar of 0.70 means *no evidence of an edge*, which the maximum alone would have hidden.

### Out-of-sample split

Split the data into an **in-sample** head and an **out-of-sample** tail; the strategy runs on both and the two stat blocks (return, profit factor, win rate, max drawdown, trades) are shown side by side. A large gap between the columns is a sign of overfitting.

### Results

- Headline stats: return (vs **buy & hold**), net PnL and fees, win rate, profit factor, expectancy, max drawdown, Sharpe/Sortino.
- **Equity curve** overlaid on price with entry/exit markers.
- A full **performance summary** (gross profit/loss, payoff ratio, largest win/loss, max consecutive wins/losses, average bars in trade…) and the complete **list of trades** with per-trade **MAE/MFE** (worst open loss / best open profit while in the trade), filterable, and exit reasons (signal faded, exit signal, stop-loss, take-profit, reversed, data end).
- **Save runs** by name and keep a history to compare strategies later; each saved run has a downloadable **Markdown report**.

## Quant Tools {#quant}

Risk/return analytics on your datasets and trade history, in six tabs:

### Single Asset

Pick a dataset and a time range, and get the risk profile: **historical volatility** (annualized), **max drawdown**, **Value at Risk** and **Conditional VaR** at your chosen confidence, plus drawdown and return-distribution charts.

### Portfolio

Select 2+ datasets with the same timeframe:

- **Correlation matrix** — are you diversifying, or buying the same asset twice?
- **Efficient frontier** — a cloud of random allocations; click the highlighted **max-Sharpe** or **min-volatility** points to read their weights.
- **Risk parity** — weights that size volatile assets smaller so no single name dominates risk.

### Position Size

The everyday one: given your stack, entry, stop and risk, it computes **position size, notional, margin needed, exposure and reward:risk**. It can also **suggest stops** from a dataset — volatility-, ATR- and swing-based — and fill entry from the last close.

### Kelly

From win rate and payoff (or avg win/loss), computes the **Kelly fraction** with half- and quarter-Kelly variants. Full Kelly maximizes long-run growth but is volatile; most traders size at half or quarter Kelly.

### Monte Carlo

Pick a **saved backtest run** and resample its realized trade sequence thousands of times to see how much of your result was skill versus luck. You get **percentile bands** on final equity and max drawdown (a fan chart of the paths and a distribution chart), plus a **risk of ruin** — the fraction of paths whose equity ever fell to a threshold you set — with your real, un-resampled equity curve overlaid for reference.

### Seasonality

Over a single dataset, a set of heatmaps of **average return** by **month of year**, **day of week** and **hour of day**, each cell showing the mean and its sample count — so you can spot recurring calendar patterns. The hourly clock is **UTC**.
