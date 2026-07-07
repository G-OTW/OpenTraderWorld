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

*Combine indicator signals, size with pyramiding, measure the edge.* Pick a dataset, define rules, run — no code.

### Strategy

- **Entry / exit rules** per side, built from comparisons between indicators, price and fixed values. Group rules with **AND** (all must hold) or **OR** (any one is enough).
- **Direction** — long, short, or both. Options: derive the short side as the inverse of long, and **stop & reverse** (flip position when the opposite signal fires).
- **Stop-loss / take-profit** per side; with no exit rules, exits happen via SL/TP or reversal.

### Sizing, account & costs

- Size by **percent of equity** or **fixed quantity/lots/contracts**, with **leverage** and **starting capital**.
- **Pyramiding** — allow up to N stacked entries when the entry signal re-fires; SL/TP then track the average entry price.
- **Costs** — fee (fixed or % of notional, per trade or per unit) and **spread %**, so results aren't fantasy.

### Results

- Headline stats: return (vs **buy & hold**), net PnL and fees, win rate, profit factor, expectancy, max drawdown, Sharpe/Sortino.
- **Equity curve** overlaid on price with entry/exit markers.
- A full **performance summary** (gross profit/loss, payoff ratio, largest win/loss, max consecutive wins/losses, average bars in trade…) and the complete **list of trades** with exit reasons (signal faded, exit signal, stop-loss, take-profit, reversed, data end).
- **Save runs** by name and keep a history to compare strategies later.

## Quant Tools {#quant}

Risk/return analytics on your datasets, in four tabs:

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
