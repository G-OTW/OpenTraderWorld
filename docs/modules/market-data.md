# Market data & backtesting

These modules form a chain: **Historical Data** downloads price history into local datasets; **Backtest** and **Quant Tools** work on those datasets, and **Visualization** charts any instrument a connector serves, stored or not. All three require Historical Data to be installed, since it owns the dataset catalogue they read.

## Historical Data {#histdata}

Download OHLCV candles from external providers into datasets stored in your database, or [import a file](#import) you already have. Once stored, the data is yours: chart it, backtest it, export it, no re-fetching.

### Providers & credentials

Providers are configured once, centrally, as **[data connectors](/config/connectors)**: a named provider account with its credentials, an optional request limit, and the modules allowed to use it. Historical Data has **no provider settings of its own**: the *Connectors* button next to the provider picker opens the same shared screen you'd find in Settings.

Some providers are **keyless** (Binance and Binance futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) and work immediately; others need an API key, and most have free tiers. A connector missing its credentials shows *needs credentials* and is skipped until you set them.

Outbound calls are counted in **Settings → API rate** so you can watch your free-tier usage.

### Downloading

Pick provider, asset type, timeframe, ticker and date range, then **Download**. Notes:

- **Futures** use contract codes: base + month letter + year digit (`F G H J K M N Q U V X Z` = Jan…Dec), e.g. `GCJ5` for April 2025 gold.
- **Options** are built from underlying, expiry, call/put and strike.
- **Intraday limits**: providers only serve intraday granularity for a limited lookback (e.g. ~7, 60 or 730 days depending on provider). Older history is available at **1d / 1w** without limit. The form warns you before you queue an impossible range.

### Several at once

One form queues a whole batch: tick **as many timeframes** as you need and type **several tickers separated by commas** (`BTCUSDT, ETHUSDT, SOLUSDT`). One download is queued per symbol × timeframe pair (3 symbols × 2 timeframes = 6 downloads), all on the same connector and the same date range.

Before you press Download, the form **prices the batch**: how many downloads it is, roughly how many provider requests that costs, and the minimum time it will take (they run one after another to respect the provider's rate limits). When the connector carries a [request limit](/config/connectors#request-limits), a small gauge shows how much of the current window is already spent, and the line warns you when the batch goes past it. It is not blocked: the rest waits for the quota to reset and resumes on its own.

### Watching the jobs

Downloads run as background **jobs**, chunk by chunk, with live progress. Filter jobs by status, provider, timeframe or ticker; a batch is grouped under one header showing how many of its downloads are done.

- **A job that hit a limit is `waiting`, not failed.** The row says why (*quota reached* or *provider rate limit*) and counts down to the moment it resumes by itself.
- **Cancel** stops any unfinished job, and one click cancels **the rest of a batch**. Cancelling is cooperative: the worker stops at the next chunk boundary and the bars already written are kept.
- A long park and the end of a batch raise a notification, pushed to the [channels](/config/settings#notifications) Historical Data has been granted.

### Datasets

The **Datasets** tab lists everything stored: bar counts, date range, size. From here you can:

- **Fetch newer**: pull bars newer than the last one stored (top up a dataset).
- **Export** as **CSV** or **Parquet**. CSV opens in any spreadsheet; Parquet is the same bars typed and compressed, roughly a tenth the size, read by `pd.read_parquet` with no date parsing or dtype guessing. The Parquet file also carries the instrument, timeframe and source in its own metadata, so importing it back anywhere in the app fills the form by itself.
- **Delete** a dataset (drops all its bars).
- Jump straight to a **chart** of it.

Imported datasets sit in the same list, named after where their file came from rather than after a provider. They carry no *Fetch newer* button: there is no provider behind them, and the way to extend one is another file.

### Importing your own file {#import}

**Import** on the Datasets tab reads price history you already have: an exchange dump, a broker export, a spreadsheet, a vendor's archive. CSV, TSV, TXT, JSON or **Parquet**, up to 20 MB, one row per bar.

The file never leaves your browser between steps and is never stored on the server: each step ships it again, so there is no half-finished upload to resume or clean up.

**The columns are proposed, you confirm them.** Headers are matched against a multilingual dictionary (six languages) *and* against what the values actually look like, so `Date;Ouverture;Plus haut;…` and `open_time,open,high,low,close,volume` both land mapped. A dot next to each column says how sure the detector is; anything it is unsure about is left for you. Correcting a column teaches it: the next file with that header maps itself.

A Parquet file is read into the same grid as a CSV, so column detection, the mapping step and the preview work identically. Its types are honoured: a legacy INT96 timestamp (what Spark and older pandas write) and a `DATE` become dates, a `DECIMAL` keeps its scale, a null stays an empty cell. The same reader serves the Journal and Portfolio imports, so those accept Parquet too.

**Timestamps** are read as dates or as Unix integers in seconds, milliseconds, microseconds or nanoseconds, detected per column and overridable. A text timestamp that carries no timezone is read at the offset you pick, which decides *which period* each row belongs to, not merely how it is displayed.

**What is missing is filled, never invented.** A file with a single price column is a close series (a NAV, an index level): open, high and low are filled from the close, making a flat bar, and the form says so. A column you *did* map that is empty on a row is an error naming its line, not a zero.

Before anything is written, the preview reports over the whole file: bars, rows, columns, the first and last date, the spacing your timestamps actually have (offered as the timeframe), periods missing at that spacing, rows sharing a period, bars whose high/low do not contain their open/close, and every row that could not be read.

**What the file cannot say, you say.** A file says "Close"; it does not say the bars are AAPL daily. So the import asks for:

- **Ticker**, **asset type** and **timeframe** (the timeframe is pre-filled from the file's own spacing).
- **Source**: the broker, venue or vendor the file came from, free text. It is *part of the series identity*, so the same instrument exported by two brokers stays two datasets instead of two tapes averaged into one.
- **Name** and **tags**: your own labels, used to find the series again in the catalogue and to filter it.

**Importing twice is safe.** A re-import lands on the same dataset and overwrites period for period: same file, same result. Overlapping periods are counted in the preview before you commit.

A Parquet file exported from here skips most of that form: it already knows its ticker, asset type, timeframe and source, and only fills the boxes you left empty, so anything you typed still wins. Export, edit in pandas, import back.

Once imported, the series is an ordinary dataset: backtests, Quant Tools, journal enrichment and the portfolio's factor proxies read it like any downloaded one.

### Look before you download

You don't have to queue a job to find out whether a symbol is worth storing. The chart fetches a window through a connector and **stores nothing**; when the window looks right, **save** it and the normal download job is queued for exactly that range. Saving over bars you already hold consolidates rather than duplicating, so topping up a dataset from the chart is safe.

## Historical Data Visualization {#histviz}

The chart is not bound to a dataset: it opens an **instrument**. Search a symbol, pick a timeframe, and the bars arrive whether you have downloaded them or not: the server serves what is already in your catalog and fetches only the missing edges through a [connector](/config/connectors). Nothing is written unless you ask.

The page is a **workspace**: a grid of charts, an instrument list beside them, and one quick-backtest session under them. Everything below describes a single chart unless it says otherwise; the grid itself is in [Workspaces](#workspaces).

### Finding an instrument

Nothing is written over the candles that does not belong to them: the **symbol at the top left of a chart is a button**, and it opens the instrument picker as a modal, one search box over every connector the chart is allowed to use. Type `BTC` and Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca and Massive answer together, each hit labelled with the connector that served it. Filter by asset type; tick or untick sources in the same modal (the tick **is** the grant, and the server refuses a connector this module was never given).

With the box empty the panel lists what you charted **recently**, then what is already **stored**, both opening in one click and costing nothing.

A row you cannot chart says so in place of the chart, naming the reason: the connector was never granted, the provider does not know the symbol, a credential is missing, or no granted connector serves it at all. Each message carries the button that fixes it.

### Workspaces {#workspaces}

A workspace is a **grid of charts**, from 1x1 up to 3x4. Rows and columns are picked from the toolbar, so a vertical split, a horizontal one and a 2x2 are the same control rather than a list of named layouts; drag the splitters to give one chart more room, or maximize a chart and come back to the grid. Keep as many workspaces as you like, name them, and switch from the picker; the one you had open comes back on reload.

Each chart carries its own instrument, timeframe, plot style, indicators and drawings. A chart is closed from its own header, and an empty cell asks for an instrument.

**Link groups.** Click a chart's link button to give it a colour. Charts sharing a colour share the **symbol** and the **crosshair**, and the visible span too when they are on the same timeframe. The timeframe itself is deliberately never shared: three panes on one symbol at 1m, 1h and 1d is the reason to link them at all.

**One connection for all of them.** Panes on the same account share a single live stream, so four charts on one Alpaca key spend one connection seat, not four.

### The instrument list {#rail}

A rail down the left of the page, from two sources that never get mixed up:

- **Chart lists** are the rail's own, built with the `+` button, which opens the same instrument picker. They hold chart coordinates, so a click charts them with no lookup. **Recent** is the same thing without a name.
- **The Watchlists module's lists** hold quote symbols, so charting one is a lookup. When the list or the row quotes through a data connector, that connector is the only one asked: a symbol quoted through IBKR charts on IBKR or not at all.

Click a row to chart it in the active pane, or drag it onto any pane. **A watchlist is never written to from here**: editing one copies it into a chart list first and edits the copy, and turning a chart list into a real watchlist is the **Promote** button and nothing else.

### Loading history

The chart opens on the newest **1500 bars** and puts a button at the left edge of the loaded data. Each click walks one slice further back. No provider request ever happens without a gesture from you, which is what keeps a metered key predictable; the walk stops when the provider's history runs out.

The **timeframe** dropdown offers every bar size the connector supports, not only the ones you happened to download, and switching timeframe **keeps the dates you were looking at**.

### Live streaming {#live}

For a stream-capable connector, the chart **goes live by itself** as soon as the newest bar is
the one forming now: the last candle updates in place, and the control shows the connection
state and the lag behind the exchange. The same control stops and restarts the feed by hand.

Live is addressed by **instrument**, not by dataset, and **stores nothing**. Any symbol a
streaming provider serves can be watched live without downloading it first, which is the point:
you can look at something before deciding it is worth keeping. Save the instrument while it is
live and the same feed starts recording closed bars into the dataset as well.

**Who streams what.** Binance (spot and futures), Bitget, OKX, Kraken and Coinbase stream every
timeframe they download, daily included, because on a 24/7 market the daily candle is the epoch
day. Capital.com streams every timeframe too, from the bid and the ask candles it publishes
separately, charted at their mid. Alpaca, Massive and
Interactive Brokers publish **one grain each** (one-minute bars, one-minute aggregates,
five-second bars) and the chart folds your timeframe from it. That covers every **intraday**
timeframe and leaves daily and weekly to the download: an equity session is not 1440
epoch-aligned minutes, so a daily candle built that way would disagree with the stored one. On
a timeframe that cannot stream, the control says which ones can instead of vanishing. The
per-provider detail is in [live streaming](/config/connectors#live-streaming).

A chart streams one symbol whatever the number of panes: several panes on one instrument, and
several panes on one account, share the connection rather than each taking a seat.

**Which account.** When a provider holds more than one connector, the live control grows an
account picker. Two keys are two entitlements and two connection seats, so which one gets spent
is your call, not a fallback. Switching restarts the feed on the other one.

#### No gap at the seam

Loading the window, opening the socket and waiting for the provider to publish all take time,
and a minute-bar provider only speaks once a minute. By the time the first live tick lands, the
chart can be one or several candles behind, and those candles used to be missing until you
reloaded.

So the stream tells the server where the chart ends, and the server sends what closed in
between: from the catalog when the instrument is stored, from the provider only for the tail it
cannot answer, nothing at all when there is no gap. What you see going live is what the market
did, without a hole at the join.

#### When it cannot run

A rejected key, a plan without streaming, an instrument your account has no subscription for:
these are answers, not faults. The chart names which one it is, quotes the provider's own
words, and **stops** rather than reconnecting forever behind a dot that never turns green.

| What you see | What it means | What to do |
|---|---|---|
| *Not authorized* / rejected key | the credentials are wrong, or the plan has no live feed (a free Massive key downloads history and is refused at the live login) | fix the connector, then press *Try again* |
| *No subscription* for this symbol | the account is connected but not entitled to that instrument's feed (Alpaca free streams IEX, not SIP or OPRA; IBKR serves what you subscribe to) | pick another instrument or add the subscription at the vendor |
| *Connection taken* | most vendors allow one live connection per account, and another program is holding the seat | close the other program; this one keeps retrying on its own, since it clears by itself |
| *This timeframe does not stream* | the provider publishes a single grain and your timeframe is above it | switch to one of the timeframes the control lists, or stay on downloaded data |
| *Connector not granted* | the chart was never given this connector | grant it in [Data connectors](/config/connectors), from the link in the message |
| *Market data lines* nearly all used | Interactive Brokers caps how many symbols an account streams at once, and the workspace is close to that cap | close a pane, or stop the live feed on one you are not watching, before the next chart goes quiet with no reason given |

Everything else (a dropped socket, a provider hiccup) reconnects quietly with a backoff.

### Chart

- **Chart types**: candles, OHLC bars, line, and **Renko** (with brick size).
- **Indicators**: SMA, RSI, MACD and more, as overlays or separate panes, each with configurable source, line/fill colours and width. Every series gets **its own line in the chart header**, carrying hide, settings and remove on hover, and each pane is titled over the drawing it holds. The header **reads at the crosshair**: O H L C, the change, and every indicator's value at the bar under the cursor, falling back to the newest visible bar when the cursor is away.
- **Chart settings**: linear or logarithmic scale, horizontal and vertical grid lines, **day separators**, the **previous close** drawn as a reference line, crosshair and its per-series value tags, hover tooltip (off by default), up/down colours, price scale on the left or the right, and a **last-price tag** pinned on the price axis, tinted like the candle that produced it.
- **Navigation is hand-driven**: drag to pan both axes (time sideways, price up and down), wheel to zoom (calibrated per device, so a mouse notch and a trackpad tick move the same amount). Changing chart type keeps the current zoom.
- **Volume** is drawn at the bottom of the price pane, the way market terminals draw it, rather than in a pane of its own. Plus a single fullscreen mode.
- **Micro-cap prices** are written `0.0₅4549`, the subscript counting the zeros, instead of a scale of identical `0.0000` labels.

### Comparing two instruments

Add another instrument to the same chart and it is drawn **rebased**, since two prices in two currencies on one axis say nothing. Two readings, one click apart:

- **percent change**, both series rebased to the start of the window, which answers *which one went up more*;
- **ratio**, this instrument divided by the other, rebased to 100, which is the pair-trade view: the line rises when the one you are charting outperforms.

Comparison series are aligned to the chart **period by period**, so two markets that stamp the same day differently still line up.

### Saving the chart

- **As an image**: a PNG of the chart exactly as it is on screen, drawings and overlays included.
- **As a page**: one HTML file that opens offline in any browser, holding the picture, what it is a picture *of* (instrument, window, timeframe, indicators, comparisons, who served the bars), and **the bars themselves**, embedded. A screenshot pasted into a document is a claim nobody can check later; this one can be re-read. Nothing is uploaded: the file is built in your browser.

### Custom indicators on the chart

The indicator dialog has a second tab: **Custom**. It holds the same node-graph library the [Backtest](#strategies-and-custom-indicators) module builds from, so an indicator exists **once** and both modules see the same definition. Pick one from the list to plot it, or build a new one right here with the same builder; saving writes it back to the shared library.

Unlike a catalog indicator, a custom one **chooses its own pane**: on the price, or in a pane of its own. That choice is yours per instance, so the same indicator can overlay the candles on one chart and sit below them on another. A 0-100 indicator drawn as an overlay gets a hidden second scale, so it cannot flatten the price.

The definition **travels with the instance**: a chart still draws its custom indicator after the library row is deleted, and reloading refreshes it from the library while the row exists.

### Drawing tools

A rail against the chart's left edge: **trend line**, **horizontal** and **vertical** line, **rectangle**, **Fibonacci retracement**, **text**, **long** and **short position** boxes (entry, target and stop, with the resulting R:R), and a **measure** reporting the price change, the percentage, the number of bars and the elapsed time.

- Each object has its own **style** (colour, width, dashes) and is edited by dragging its handles.
- An **OHLC magnet** snaps a handle to the open, high, low or close of the bar under it, and a handle dropped near an object already on the chart snaps onto it, with a guide line saying what it caught.
- **Style templates**: style one object, save it under a name, and apply it to the next ones. One template can be made the default for every new drawing.
- **Copy between charts**: <kbd>Ctrl/⌘+C</kbd> then <kbd>Ctrl/⌘+V</kbd> pastes the selected object, into another instrument too; <kbd>Ctrl/⌘+D</kbd> duplicates it in place, offset by a bar.
- Drawings are anchored in **time and price**, not in pixels, so they stay on their bars through any zoom, pan or timeframe change.
- They are kept **per instrument**, not per timeframe and not per pane: a trend line drawn on the 1h is the same line on the 15m, and two panes on one symbol show one board.
- **Undo** (the rail's button, or <kbd>Ctrl/⌘+Z</kbd>) takes back the last drawing, or the last quick-backtest order.

#### The objects list

Past five drawings a chart needs a list, so there is one: every object on this instrument with what it is and the price it sits at, and the four things it then needs, **hide**, **lock**, **delete** and **reorder**. The order is the paint order, which decides what sits on top. Selecting a row selects it on the chart, and the list is the same array the chart draws, so an edit shows before the dialog closes.

### Alerts {#alerts}

A price or an indicator level, **watched by the server**. The browser can be closed, the machine can be doing something else: the alert fires anyway, into your notifications and into the [channels](/config/settings#notifications) the chart is granted (none ticked = every one of them).

Set one from the alerts dialog, or from a horizontal line you have already drawn, which hands over its price.

Three rules are worth knowing, because they are decisions rather than details:

- **Closed bars only.** A forming candle's high is not a fact yet, it can be revised by the next tick. An alert that fired on it would be reporting something that never happened.
- **A crossing, not a state.** *Crosses above* waits for the price to come **through** the level going up, so an alert placed under the current price does not fire the instant you create it.
- **The level is read on this chart's timeframe**, and a daily alert is re-read far less often than a one-minute one: an unstored instrument costs one small request per check, and a bar that moves once a day does not deserve one a minute.

Each alert can fire **once** or every time, with a cooldown. The list says when each one last fired, what it last read, and, when a connector or a quota gets in the way, why it could not run at all. Alerts are paused and armed again from that list.

### Broker book {#broker-book}

Sync a [broker account](/config/brokers) and the chart draws what you actually hold: a price line per open position at its average cost, one per working order at its limit or stop. Levels land on the chart whose ticker matches, punctuation aside, so a workspace of several instruments annotates itself. Read-only, and re-read only when you press *Sync*: a position with no average cost gets no line and is counted as such instead of being placed somewhere plausible.

### Quick backtest

A scratchpad for trading a chart by hand: **click the chart to open a position, click again to close it**. Long-press instead to choose the side and the size, and click a marker's arrow to flip it. Pyramiding, partial closes and reversals all follow from the fills you place.

The session spans **the whole workspace, not one chart**: every chart on screen posts its fills into it and the numbers are their sum, the way a book of several instruments actually reads. The picker in the panel names the **target chart**, the one a click places a trade on and the one the size box edits; it is the active pane, so picking here and clicking there are the same act.

The strip under the workspace is one line of session figures when collapsed. Expanded it is resizable by its top edge and has three tabs:

- **Trades**: the session's trade list with entry, exit, P&L and R (measured against the trade's worst open loss, since there is no stop to quote), plus a reset.
- **Statistics**: win rate, expectancy, profit factor, results per side, streaks and a distribution of returns.
- **Performance**: the session's P&L curve, each trade carrying its run-up and its worst open loss, so a winner that spent the day underwater reads as one.

Size is entered in **units**, **contracts** (multiplied by a point value) or **notional**, and converted at the fill price. These fills live **in your browser, per instrument**: it is a scratchpad for reading a chart, never journal data, and nothing is posted to the [Trading Journal](/modules/journal).

The **Backtest** button hands the same instrument to the [Backtest](#backtest) module, saving it first if it wasn't stored.

### The chart remembers where you left it

Chart type, indicators and drawings belong to the **instrument**, not to a dataset and not to a pane: they are saved server-side under the symbol's own coordinates shortly after each edit, and they come back the same way in any pane, in any workspace, from any browser. That holds for a symbol you only looked at once and never downloaded, which is the point: charting something you have not decided to keep no longer means losing what you drew on it.

What is *not* stored server-side is the quick-backtest session, which stays in this browser.

The **autosave data** switch in the chart settings is a separate decision, about the bars rather than the layout: it stores an instrument the first time you chart it, which queues a download. It is off by default.

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

*Combine indicator signals, size with pyramiding, measure the edge.* Pick one dataset or a whole portfolio, define rules, run. No code.

### Strategy

- **Entry / exit rules** per side, built from comparisons between indicators, price and fixed values. Group rules with **AND** (all must hold) or **OR** (any one is enough).
- **Direction**: long, short, or both. Options: derive the short side as the inverse of long, and **stop & reverse** (flip position when the opposite signal fires).
- **Stop-loss / take-profit** per side: each is a checkbox you turn on or off independently (percent of average entry, or an ATR multiple). With no exit rules, exits happen via SL/TP or reversal.

### Sizing, account & costs

- Size by **percent of equity** or **fixed quantity/lots/contracts**, with **leverage** and **starting capital**. Fixed quantity **scales with leverage**, following the retail convention, so a leverage of 3 on a fixed size of 1 opens 3 units.
- **Pyramiding**: allow up to N stacked entries when the entry signal re-fires; SL/TP then track the average entry price.
- **Costs**: fee (fixed or % of notional, per trade or per unit) and **spread %**, so results aren't fantasy.

### Sizing (advanced)

Beyond percent-of-equity and fixed quantity:

- **Risk per trade**: size so a stop-loss hit costs a fixed % of equity (needs a stop on the traded side).
- **Fractional Kelly**: size from the win rate and payoff of the last *N* closed trades, scaled by your chosen fraction and capped; a warm-up size is used until the window fills.
- **Equity tiers**: a table of thresholds; the highest tier whose level is ≤ current equity sets the size.

### Portfolio (multi-asset)

Add several datasets and run one strategy across all of them on a **merged clock** (all locked to the same timeframe):

- An **alignment preview** shows the merged-clock length, the overlapping window, indicator warm-up bars (including the cumulative lookback of a chained custom indicator), and per-asset missing bars, all before you simulate.
- **Portfolio limits**: cap the number of open positions and total / per-asset exposure.
- A **per-asset breakdown** reports trades, net PnL, fees, win rate and exposure for each instrument.

### Grid strategy

A ladder of price levels between a lower and upper bound; each cell buys low and sells at the next level up, **long**, **short** or **neutral**. Size a fixed quantity per level or split a total budget across cells, with optional stops above/below the ladder. Results report fills, round trips and end inventory.

### DCA (savings plan)

A third mode next to signal rules and the grid, for the way most money is actually invested: a **weighted basket**, bought over time, never rebalanced.

- **Weights, fixed.** Every euro deployed is split by the weights you set per ticker. Nothing is rebalanced, so a rule that fires on one asset of five deploys that asset's own share.
- **Money in.** The starting capital is bought in one shot at each asset's first bar. Everything after it is **new money**: a recurring contribution (per bar, day, week, month, quarter or year, invested on arrival or kept as cash), and buy rules that pay an amount in when their condition holds.
- **Buy rules**: a fixed amount, a % of cash, of the portfolio or of the cost basis, with a maximum number of fires and a cooldown, filled at the next bar's open.
- **Sell rules**: a % of the position, the whole position, a number of units or an amount, triggered by a **gain objective**, a condition, or both, with the proceeds kept as cash or withdrawn.
- **Conditions** are the engine's ordinary rule groups plus two families written for this mode: **market metrics** (drop from the high, rise from the low, change over N bars, change since the start) and the **live position** (P&L %, drift since the last buy, average cost, units, value, weight %, cash %, drawdown). They are evaluated on a **weighted basket index** by default, or per asset, which then buys only the assets that hold.
- **Measures for a savings plan**, not for a strategy: drawdown and Sharpe on the **deposit-adjusted (time-weighted)** curve, so a deposit is not read as a rally; return on the money in; **IRR** for the money-weighted return; and a benchmark of the same total contributed deployed in one lump sum at the start.

Sizing, pyramiding and stops do not apply here: the plan's own rules decide every fill.

### Trading window

A **Filters** step decides *when* the strategy may open, on a fixed UTC offset you choose (no daylight saving, which is what an exchange session rule means):

- **Weekdays** and **sessions** (several per day, an end before its start wraps past midnight).
- **Calendar**: trade only on given dates, or never on them. *Never* wins over *only*.
- Outside the window the position is either **kept** or **closed**, and pyramiding adds can be blocked too. Entries are gated; exits, stops and take-profits keep running on every bar.

### Costs & execution realism

- **Slippage**: a fixed number of ticks or a percent of price, applied to every fill.
- **Funding**: a constant annual rate on open notional for perp estimates (longs pay, shorts receive).
- **Circuit breakers**: halt trading after a max daily loss (for the day) or a max drawdown (for the run).
- **Instrument profile**: tick size, lot step, minimum quantity and contract multiplier, so sizes and prices snap to a realistic contract.

### Strategies and custom indicators

- **Named strategies**: save, search, duplicate and edit full strategy configurations.
- **Custom indicators**: build your own from named steps, no code. Each step either applies a built-in indicator to a **source** (a price field or the output of an earlier step) or computes a **formula** referencing earlier steps by name (`@volume / SMA(@volume)`, with `+ − × ÷`, `min`, `max`, `abs`, `clamp`). This lets you chain indicators: a Hull MA of an RSI, a MACD of an RSI, a smoothed volume ratio, and so on. Indicators that read full candles (ATR, Stochastic, ADX, VWAP…) only apply to the price, not to a derived step. The highlighted step(s) are the output. Custom indicators become operands in the rule editor alongside the built-ins, and the library is **shared with the chart**, which draws the same definition ([Custom indicators on the chart](#custom-indicators-on-the-chart)).
- **Searchable indicator picker**: pick indicators from a grouped, type-to-filter list (in both the rule editor and the custom-indicator builder) instead of scrolling one long dropdown.

### Date windows and parameter sweeps (API)

Two capabilities live on the API rather than in the form. They exist for the [assistant](/modules/agent) and for anyone driving the app over [MCP](/config/ai-agents):

- **Date-windowed runs.** `from` / `to` on a run (and on the alignment preview) restrict the simulated span, which is what walk-forward validation and regime slicing need: run 2019–2021, then 2022–2024, and compare. `to` is inclusive of the whole day.
- **`POST /api/backtest/sweep`**: run a parameter grid server-side and get **every trial back**, along with the trial count. Grid paths reach into arrays (`long.entry.conditions.0.left.period`), so indicator periods are sweepable; capped at 4 axes and 64 trials.

A sweep also returns a **deflated Sharpe**: the Sharpe the best of N *worthless* strategies would be expected to reach, given how much these particular trials varied. Compare the winner against that bar, not against zero: on real daily bars, the best of eight moving-average crossovers scoring 0.59 against a selection bar of 0.70 means *no evidence of an edge*, which the maximum alone would have hidden.

### Optimizer

Take a finished run and **vary its parameters**: indicator lengths and thresholds per side, stops, sizing, costs, portfolio limits, grid settings, and which weekdays to exclude (every subset is tried). Each parameter gets a from / to / step, and the header counts the variants as you widen them, capped so a grid stays finite.

- **Before it starts**, it estimates the cost from what past runs measured on your machine: milliseconds per variant, workers, total time. You can stop at any point and keep what has been computed.
- **Ranking** on the metric you pick (Sharpe, Sortino, return, profit factor, win rate, expectancy, max drawdown, trades, out-of-sample return); any column re-sorts afterwards.
- **Analysis** shows the spread of the chosen metric across every variant: worst, average, best, and how many came out positive. A single good number means little if its neighbours are terrible.
- **Multiple-testing haircut**: the best Sharpe is shown against the **selection bar**, the Sharpe the best of that many *worthless* strategies would be expected to reach. Below the bar, trying this many variants is enough to explain the winner.
- Click a variant to read its **full backtest**, replayed from its own settings. Nothing is stored until you **keep** it in the history.

### Out-of-sample split

Split the data into an **in-sample** head and an **out-of-sample** tail; the strategy runs on both and the two stat blocks (return, profit factor, win rate, max drawdown, trades) are shown side by side. A large gap between the columns is a sign of overfitting.

### Paper trading

*A finished run, left running forward.* Press **Paper trade** on a result and the strategy keeps trading on paper, on a schedule, alerting the channels you pick. There is no second engine: every run re-simulates the window with the ordinary backtest and reports what changed, so a paper fill is by construction the fill the backtest would have shown for the same candles.

- **The strategy is frozen** as it ran, instruments included. Editing that strategy afterwards does not change a running session; the session's own form offers to update it or to start a copy when you save the strategy again.
- **The first run seeds the book.** Every round trip already in the window is recorded at once, summarized in a single event: opening a session does not fire a burst of alerts about history. Only what happens after that is a fill worth a message.
- **Window**: how much history each run feeds the engine (trailing candles, or a pinned start).
- The **Paper** tab lists the sessions with their status, next run, open positions and unread events, and each can be run now, paused, resumed or deleted. The event log is kept whether or not anything was sent, so what could not be delivered is still there when you come back.

#### Schedule

Every run, and its data, is on the candle's own clock.

- **Interval** (every N minutes), **daily**, **weekly**, **monthly** or **once**, in a real timezone.
- An interval fires on the **candle grid**, never on the second the session was created: every minute at :00, every 15 minutes at :00 / :15 / :30 / :45, hourly on the hour.
- The run **downloads the candle it waits for** into the session's own datasets. The candle in progress is never stored: what is read is the last *closed* one. A candle the provider has not published yet is asked for again over a couple of seconds, and a period with no trade at all writes nothing, which simply means the next run has nothing new to simulate.
- A run that finds no new candle costs nothing: it does not simulate at all.

#### What is sent, and where

- **Notify**: on every run, only on a new fill, or never. *Every run* also reports the quiet ones, which is the only way to tell "nothing happened" from "the engine stopped running".
- **Group messages**: send immediately, or an hourly, daily or weekly digest. A grouped send is one message about a period, not one ping per fill.
- **Alert on** entries, exits, or both, and optionally only for the **instruments** you name.
- **Channels**: the [notification channels](/config/settings#notifications) Backtest has been granted, all of them or the ones you pick. With no channel set up nothing is sent, and every event is still recorded in the app.

#### Custom messages

Three messages, three wordings, each with its own vocabulary: **Entry**, **Exit** and **Summary** (the grouped one). Leave a field empty and the built-in wording is used.

- Each has a **Title** and a **Body**, written with placeholders:

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variables** lists exactly what that message can read, with a sample value; click one to insert it. An entry reads the entry half of the trade, an exit the whole of it (P&L included), and the summary reads the period, `since.*` (since the last alert), `total.*` (since the session started) and `open.*` (what is held right now), plus `session.*`, `event.*` and `stats.*`. A path a message cannot read is not offered to it.
- **Filters** are written `| name:arg`, and `upper`, `lower`, `trim` and `json` take none:

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- The **Preview** is the server's own renderer, so what it shows is what would be sent. It runs on a sample trade and on the session's last figures, so a value the session has not produced yet is marked in place, in brackets, and the rest of the message still renders.

### Results

- Headline stats: return (vs **buy & hold**), net PnL and fees, win rate, profit factor, expectancy, max drawdown, Sharpe/Sortino.
- **Equity curve** overlaid on price with entry/exit markers.
- A full **performance summary** (gross profit/loss, payoff ratio, largest win/loss, max consecutive wins/losses, average bars in trade…) and the complete **list of trades** with per-trade **MAE/MFE** (worst open loss / best open profit while in the trade), filterable, and exit reasons (signal faded, exit signal, stop-loss, take-profit, reversed, data end).
- **Save runs** by name and keep a history to compare strategies later. The **Reports** menu of a finished run exports it whole, per side and per asset: a **PDF** with every chart, or **Markdown** with the figures alone. Neither carries the trade list; the file is named after the strategy and the moment it was exported.

## Quant Tools {#quant}

Risk/return analytics on your datasets and trade history, in six tabs:

### Single Asset

Pick a dataset and a time range, and get the risk profile: **historical volatility** (annualized), **max drawdown**, **Value at Risk** and **Conditional VaR** at your chosen confidence, plus drawdown and return-distribution charts.

### Portfolio

Select 2+ datasets with the same timeframe:

- **Correlation matrix**: are you diversifying, or buying the same asset twice?
- **Efficient frontier**: a cloud of random allocations; click the highlighted **max-Sharpe** or **min-volatility** points to read their weights.
- **Risk parity**: weights that size volatile assets smaller so no single name dominates risk.

Mixing asset classes is fine, including different providers: bars are matched by the **period** they belong to, not by the timestamp the provider stamped them with (a daily crypto candle opens at 00:00 UTC, a US equity one at the New York session start, and both are the same day). Two things follow, and the panel says which one applied:

- A basket that mixes a 24/7 market with an exchange-hours one is measured **weekly**. Aligned daily, the weekend move of the continuous asset would land on the same row as the other's Monday and understate how much they really move together.
- Annualization is **counted on the clock** rather than assumed: the same daily datasets are 252 periods a year on an exchange and 365 on a 24/7 market.

Intraday datasets are the exception: 4h bars anchored on a trading session and 4h bars anchored on the clock are 90 minutes apart, so a mixed-provider intraday basket is refused rather than approximated. Use daily datasets, or one provider for the whole basket.

### Position Size

The everyday one: given your stack, entry, stop and risk, it computes **position size, notional, margin needed, exposure and reward:risk**. It can also **suggest stops** from a dataset (volatility-, ATR- and swing-based) and fill entry from the last close.

### Kelly

From win rate and payoff (or avg win/loss), computes the **Kelly fraction** with half- and quarter-Kelly variants. Full Kelly maximizes long-run growth but is volatile; most traders size at half or quarter Kelly.

### Monte Carlo

Pick a **saved backtest run** and resample its realized trade sequence thousands of times to see how much of your result was skill versus luck. You get **percentile bands** on final equity and max drawdown (a fan chart of the paths and a distribution chart), plus a **risk of ruin** (the fraction of paths whose equity ever fell to a threshold you set) with your real, un-resampled equity curve overlaid for reference.

### Seasonality

Over a single dataset, a set of heatmaps of **average return** by **month of year**, **day of week** and **hour of day**, each cell showing the mean and its sample count, so you can spot recurring calendar patterns. The hourly clock is **UTC**.
