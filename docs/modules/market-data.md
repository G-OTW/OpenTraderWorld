# Market data & backtesting

Four modules form a chain: **Historical Data** downloads price history into local datasets; **Visualization**, **Backtest** and **Quant Tools** all work on those datasets and require Historical Data to be installed.

## Historical Data {#histdata}

Download OHLCV candles from external providers into datasets stored in your database. Once downloaded, the data is yours — chart it, backtest it, export it, no re-fetching.

### Providers & credentials

The **Settings** tab lists the available providers with their docs and rate limits. Some are **keyless** (work immediately), others need an API key — most have free tiers. Enter credentials once; they are stored encrypted. A provider that needs credentials shows *Needs credentials* in the download form until you set them.

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

## Historical Data Visualization {#histviz}

Charts any stored dataset. Pick a dataset (search by ticker/provider, filter by asset type and timeframe) and you get:

- **Chart types** — candles, OHLC bars, line, and **Renko** (with brick size).
- **Indicators** — add SMA, RSI, MACD and more, as overlays or separate panes, each with configurable source, line/fill colors and width.
- **Chart settings** — linear or logarithmic price scale, grid lines, crosshair, hover tooltip, up/down colors.
- **Volume pane**, fullscreen mode, and jump-to-start / fit / jump-to-latest navigation.

If the dataset you want isn't there yet, the picker links straight to a new download.

## Backtest {#backtest}

*Combine indicator signals, size with pyramiding, measure the edge.* Pick one dataset or a whole portfolio, define rules, run — no code.

### Strategy

- **Entry / exit rules** per side, built from comparisons between indicators, price and fixed values. Group rules with **AND** (all must hold) or **OR** (any one is enough).
- **Direction** — long, short, or both. Options: derive the short side as the inverse of long, and **stop & reverse** (flip position when the opposite signal fires).
- **Stop-loss / take-profit** per side — each is a checkbox you turn on or off independently (percent of average entry, or an ATR multiple). With no exit rules, exits happen via SL/TP or reversal.

### Sizing, account & costs

- Size by **percent of equity** or **fixed quantity/lots/contracts**, with **leverage** and **starting capital**.
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
