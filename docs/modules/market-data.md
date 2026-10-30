# Market data & backtesting

These modules form a chain: **Historical Data** downloads price history into local datasets; **Backtest** and **Quant Tools** work on those datasets, and **Visualization** charts any instrument a connector serves, stored or not. All three require Historical Data to be installed, since it owns the dataset catalogue they read.

## Historical Data {#histdata}

Download OHLCV candles from external providers into datasets stored in your database, or [import a file](#import) you already have. Once stored, the data is yours: chart it, backtest it, export it, no re-fetching.

### Providers & credentials

Providers are configured once, centrally, as **[data connectors](/config/connectors)**: a named provider account with its credentials, an optional request limit, and the modules allowed to use it. Historical Data has **no provider settings of its own**: the *Connectors* button next to the provider picker opens the same shared screen you'd find in Settings.

Some providers are **keyless** (Binance and Binance futures, Bitfinex, Bitget, Bybit, Bybit EU, Deribit, Gate.io, KuCoin, KuCoin Futures, MEXC, OKX, Kraken, Coinbase, Yahoo Finance) and work immediately; others need an API key, and most have free tiers. A connector missing its credentials shows *needs credentials* and is skipped until you set them.

Outbound calls are counted in **Settings → API rate** so you can watch your free-tier usage.

### Downloading

Pick provider, asset type, timeframe, ticker and date range, then **Download**. Notes:

- **Futures** use contract codes: base + month letter + year digit (`F G H J K M N Q U V X Z` = Jan…Dec), e.g. `GCJ5` for April 2025 gold.
- **Options** are built from underlying, expiry, call/put and strike.
- **Intraday limits**: Yahoo Finance only serves intraday bars for a limited lookback: 7 days at 1m, 60 days at 5m, 15m and 30m, 730 days at 1h. Older history is available at **1d / 1w** without limit. On a Yahoo connector, the form warns you before you queue an impossible range.

::: warning Datasets downloaded before v0.0.15
Before v0.0.15, a download or an update that reached the current time could store the candle still forming, and later updates started after it. Such a dataset can hold **incomplete candles** (high, low, close and volume cut short). Download it again to replace them. Since v0.0.15, a candle is stored only once its period has ended.

| Provider | Datasets that can hold incomplete candles |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | downloaded or updated before v0.0.15 |
| Binance (spot) | downloaded or updated before v0.0.12 |
| Binance USDⓈ-M, Bitfinex, Bitget, Bybit, Bybit EU, Deribit, Gate.io, KuCoin, KuCoin Futures, MEXC, OKX, OANDA, TradeStation, FOREX.com | none |
:::

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
- **Check gaps**: find missing candles, told apart from holidays and quiet minutes, and fetch the ones you pick after seeing the request estimate.
- **Delete** a dataset (drops all its bars).
- Jump straight to a **chart** of it.

Capital.com, OANDA and FOREX.com downloads also store the **bid and ask** of every candle; for the other providers a backtest fetches them when it [needs them](#bid-ask-providers).

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

## Charts {#histviz}

The chart is not bound to a dataset: it opens an **instrument**. Search a symbol, pick a timeframe, and the bars arrive whether you have downloaded them or not: the server serves what is already in your catalog and fetches only the missing edges through a [connector](/config/connectors). Nothing is written unless you ask.

The page is a **workspace**: a grid of charts, an instrument list beside them, and one quick-backtest session under them. Everything below describes a single chart unless it says otherwise; the grid itself is in [Workspaces](#workspaces).

### Finding an instrument

Nothing is written over the candles that does not belong to them: the **symbol at the top left of a chart is a button**, and it opens the instrument picker as a modal, one search box over every connector the chart is allowed to use. Type `BTC` and Binance, Bitfinex, Bitget, Bybit, Bybit EU, Deribit, Gate.io, KuCoin, KuCoin Futures, MEXC, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca, Massive, Capital.com, OANDA, TradeStation, FOREX.com and Interactive Brokers answer together, each hit labelled with the connector that served it. Filter by asset type; tick or untick sources in the same modal (the tick **is** the grant, and the server refuses a connector this module was never given).

With the box empty the panel lists what you charted **recently**, then what is already **stored**, both opening in one click and costing nothing.

The upload button (*Open a file on the chart*) charts a file of bars through the [import](#import) form. It is drawn for this session only, unless *Keep in Historical Data* is ticked, which stores it as an imported dataset.

A row you cannot chart says so in place of the chart, naming the reason: the connector was never granted, the provider does not know the symbol, a credential is missing, or no granted connector serves it at all. Each message carries the button that fixes it.

### Workspaces {#workspaces}

A workspace is a **grid of charts**, from 1x1 up to 3x4. Rows and columns are picked from the toolbar, so a vertical split, a horizontal one and a 2x2 are the same control rather than a list of named layouts; drag the splitters to give one chart more room, or maximize a chart and come back to the grid. Keep as many workspaces as you like, name them, and switch from the picker; the one you had open comes back on reload.

Each chart carries its own instrument, timeframe, plot style, indicators and drawings. A chart is closed from its own header, and an empty cell asks for an instrument.

A workspace is **saved on request**: *Save* (<kbd>Ctrl/⌘+S</kbd>), *Save as* and *Revert*, with a prompt before you leave unsaved changes.

**Link groups.** Click a chart's link button to give it a colour. Charts sharing a colour share the **symbol** and the **crosshair**, and the visible span too when they are on the same timeframe. The timeframe itself is deliberately never shared: three panes on one symbol at 1m, 1h and 1d is the reason to link them at all.

**One connection for all of them.** Panes on the same account share a single live stream, so four charts on one Alpaca key spend one connection seat, not four.

### The instrument list {#rail}

A rail down the left of the page, from two sources that never get mixed up:

- **Chart lists** are the rail's own, built with the `+` button, which opens the same instrument picker. They hold chart coordinates, so a click charts them with no lookup. **Recent** is the same thing without a name.
- **The Watchlists module's lists** hold quote symbols, so charting one is a lookup. When the list or the row quotes through a data connector, that connector is the only one asked: a symbol quoted through IBKR charts on IBKR or not at all.

Click a row to chart it in the active pane, or drag it onto any pane. **A watchlist is never written to from here**: editing one copies it into a chart list first and edits the copy, and turning a chart list into a real watchlist is the *Copy this list into Watchlists* button and nothing else.

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

**Who streams what.** Binance (spot and futures), Bitfinex, Bitget, Bybit, Bybit EU, Deribit,
Gate.io, KuCoin, KuCoin Futures, MEXC, OKX, Kraken and Coinbase stream every timeframe they
download, daily included, because on a 24/7 market the daily candle is the epoch day.
Capital.com streams every timeframe too, from the bid and the ask candles it publishes
separately, charted at their mid. Alpaca, Massive and Interactive Brokers publish **one grain
each** (one-minute bars, one-minute aggregates, each print) and the chart folds your timeframe
from it, daily and weekly included: those candles start where the provider's own session bars
start, and are re-read from the provider once closed. Yahoo, EODHD, Alpha Vantage, OANDA,
TradeStation and FOREX.com do not stream. On a timeframe that cannot stream, the control says which ones can instead of
vanishing. The per-provider detail is in [live streaming](/config/connectors#live-streaming).

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
| *Credentials* / *Plan* | the credentials are wrong, or the plan has no live feed (a free Massive key downloads history and is refused at the live login) | fix the connector, then press *Try again* |
| *Subscription* | the account is connected but not entitled to that instrument's feed (Alpaca free streams IEX, not SIP or OPRA; IBKR serves what you subscribe to) | pick another instrument or add the subscription at the vendor |
| *Symbol* | the provider does not know this symbol on its live feed | check the ticker, or pick another instrument |
| *Seat in use* | most vendors allow one live connection per account, and another program is holding the seat | close the other program; this one keeps retrying on its own, since it clears by itself |
| *Not supported* | the provider does not stream this timeframe or this asset type | switch to one of the timeframes the control lists, or stay on downloaded data |
| *Connector not granted* | the chart was never given this connector | grant it in [Data connectors](/config/connectors), from the link in the message |
| *Market data lines* nearly all used | Interactive Brokers caps how many symbols an account streams at once, and the workspace is close to that cap | close a pane, or stop the live feed on one you are not watching, before the next chart goes quiet with no reason given |

Everything else (a dropped socket, a provider hiccup) is labelled *Connection* and reconnects on its own with a backoff.

### Chart

- **Chart types**: candles, hollow candles, volume candles, OHLC bars, high-low, columns, line, line with markers, step line, area, HLC area, baseline, **Heikin Ashi**, and the price-driven **Renko**, **range**, **line break**, **Kagi** and **point & figure**.
- **Timeframes**: every bar size the connector serves, plus **custom** ones (7m, 10m, 3h…) built from the provider's own bars. Saving a custom timeframe stores the bars it is built from.
- **Indicators**: SMA, RSI, MACD and more, as overlays or separate panes, each with configurable source, line/fill colours and width. Every series gets **its own line in the chart header**, carrying hide, settings and remove on hover, and each pane is titled over the drawing it holds. The header **reads at the crosshair**: O H L C, the change, and every indicator's value at the bar under the cursor, falling back to the newest visible bar when the cursor is away.
- **Bid / ask**: on a live chart, the venue's best bid and ask drawn as two lines with their price on the axis, where the provider streams them (crypto exchanges, Capital.com, Alpaca, Massive, Interactive Brokers).
- **Per-chart settings**, behind the cog on each chart, with *Apply to all charts*: linear or logarithmic scale (the **A** (auto scale) and **L** (log) switches on the value scale, beside a button back to the latest bar), horizontal and vertical grid lines, **day separators**, the **previous close** drawn as a reference line, crosshair and its per-series value tags, hover tooltip (off by default), up/down colours, price scale on the left or the right, a **last-price tag** pinned on the price axis, tinted like the candle that produced it, a **last-price line**, a **countdown to bar close** and **indicator values on the scale**.
- **Navigation is hand-driven**: drag to pan both axes (time sideways, price up and down), wheel to zoom (calibrated per device, so a mouse notch and a trackpad tick move the same amount). Changing chart type keeps the current zoom. The **range bar** under the chart frames a period in one click (`1D` to `All`) at the timeframe that reads it best, and shows the current time and its UTC offset in the chart's timezone.
- **Volume** is drawn at the bottom of the price pane, the way market terminals draw it, rather than in a pane of its own. Drag the boundary between indicator panes to resize them (a double click resets). One toolbar and one drawing rail serve every chart and act on the chart in focus; fullscreen also hides the app's top bar.
- **Chart templates** save a chart's studies and layout under a name, to apply to one or several charts.
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

The definition **travels with the instance**, and reloading refreshes it from the library. Deleting an indicator or a script from the library removes it from every chart, saved workspace and template.

**Pine Script.** Paste a Pine Script v5/v6 indicator into the indicator sheet and it plots with its inputs, its warnings and per-instrument settings, saved in the same shared library. Most public indicators run, with their labels, lines, boxes, tables, fills and polylines. A script warms up on 5,000 stored bars; a chart that is not live runs it on the server and reopens without recomputing it. In a [backtest](#backtest), any plot of a script is a condition operand whose inputs the optimizer can vary.

**Futures contract specifications.** A future's tick size, point value and currency are filled in from the open-source [QuantConnect Lean](https://github.com/QuantConnect/Lean) symbol properties database (Apache 2.0), refreshed once a week, whenever its instrument settings leave them empty. The contract month is ignored (`ESZ6`, `ES.202612@CME`, `@ES` and `ES=F` are all `ES`), a root listed on several exchanges needs its exchange in the ticker, and a value you type always wins. Margins are not taken: they change by the day and every broker sets its own.

An indicator's **source** can also be another indicator, or a **formula** over price and volume, kept as a favourite for reuse.

### Drawing tools

A rail against the chart's left edge: **trend line**, **horizontal** and **vertical** line, **horizontal ray**, **parallel channel**, **rectangle**, **Fibonacci retracement** and **extension**, **text**, **callout**, **long** and **short position** boxes (entry, target and stop, with the resulting R:R), and a **measure** reporting the price change, the percentage, the number of bars and the elapsed time.

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

One series crossing another, or a fixed level, **watched by the server**. The browser can be closed, the machine can be doing something else: the alert fires anyway, into your notifications and into the [channels](/config/settings#notifications) the chart is granted (none ticked = every one of them).

Each side of the condition is what a backtest condition reads: a price field (open, high, low, close, volume), a built-in indicator, an indicator from your library (one plot of a Pine Script indicator, with its inputs), a metric, or a fixed value. So *SMA 9 crosses above SMA 21*, *close crosses below EMA 200*, *change over 5 bars above 3 %* and *relative volume (20) above 2* (the bar's volume against the average of the 20 bars before) are all one alert.

Set one from the alerts dialog, from a horizontal line you have already drawn, which hands over its price, or from the `+` on the price scale (*Alert at {price}*).

The condition is *crosses above*, *crosses below*, *crosses* (either way), *closes above* or *closes below*; the last two fire on each bar that closes past the level instead of only on the crossing. An *at time* alert compares nothing: it fires once when the clock reaches the time you set (checked every minute), then pauses itself. Set it from the `+` on the time scale (*Alert at {time}*).

Four rules are worth knowing, because they are decisions rather than details:

- **Closed bars in the background.** A forming candle's high is not a fact yet, it can be revised by the next tick. An alert that fired on it would be reporting something that never happened. When the provider streams the instrument, an alert can instead ride the live feed, while the app is open or held by the server.
- **A crossing, not a state.** *Crosses above* waits for the series to come **through** the other one going up, so an alert placed under the current price does not fire the instant you create it.
- **A script is judged when the bar closes**, live modes included, and a plot that reads ahead of its bar is refused: it would repaint, and the alert would report a crossing that later disappears.
- **The series are read on this chart's timeframe**, and a daily alert is re-read far less often than a one-minute one: an unstored instrument costs one small request per check, and a bar that moves once a day does not deserve one a minute.

Each alert fires **once**, every time, once per bar close, once per cooldown or once per market session, and can **expire** at a date (never by default). An expired alert stops being watched.

The dialog lists this chart's alerts, and **All alerts** lists every one by instrument, filtered by armed, paused, expired or in error. Each can be edited (saving arms it again), paused, armed again, deleted or opened in the chart, and *Pause all* stops them at once. The list says when each one last fired, what it last read, and, when a connector or a quota gets in the way, why it could not run at all. A notification opens the chart on its instrument.

### Broker book {#broker-book}

Sync a [broker account](/config/brokers) and the chart draws what you actually hold: a price line per open position at its average cost, one per working order at its limit or stop. Levels land on the chart whose ticker matches, punctuation aside, so a workspace of several instruments annotates itself. Read-only, and re-read only when you press *Sync*, unless trading is on for the account: then orders move and cancel from their lines and positions close from theirs (see [Trading from the chart](/config/trading)). A position with no average cost gets no line and is counted as such instead of being placed somewhere plausible.

**Entry and exit markers** (chart settings, *Trading* tab, off by default) draw the account's fills on the chart's instrument at their price and bar, a buy pointing up and a sell down, over the last 30 days (7 on Interactive Brokers).

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
| **empty** | No bars in this window. |
| **history_start** | No older bars: the provider's history starts here. |
| **provider** | Anything else the provider returned. |

## Backtest {#backtest}

*Combine indicator signals, size with pyramiding, measure the edge.* Pick one dataset or a whole portfolio, define rules, run. No code.

### Strategy

- **Entry / exit rules** per side, built from comparisons between indicators, price and fixed values. Group rules with **AND** (all must hold) or **OR** (any one is enough).
- **Direction**: long, short, or both. Options: derive the short side as the mirror of long (inverse operators, oscillator levels mirrored: RSI below 30 becomes RSI above 70; ADX, ATR and volume filters kept as they are), and **stop & reverse** (flip position when the opposite signal fires).
- **Stop-loss / take-profit** per side: each is a checkbox you turn on or off independently (percent of average entry, or a multiple of the ATR of the last candle closed before the entry). With no exit rules, exits happen via SL/TP, the trailing stop or reversal.
- **Trailing stop** per side: a percent of the best price since entry or a price distance (*Distance*), optionally held off until the trade is that far in profit (*Start at*), with an optional move to the entry price (*Breakeven at*). Its level is recalculated at each candle close and never loosens; the exit triggers inside the candle, like the stop-loss.

### Sizing, account & costs

- Size by **percent of equity** or **fixed quantity/lots/contracts**, with **leverage** and **starting capital**. Fixed quantity **scales with leverage**, following the retail convention, so a leverage of 3 on a fixed size of 1 opens 3 units.
- **Pyramiding**: allow up to N stacked entries when the entry signal re-fires; SL/TP then track the average entry price. An add is sent at the open, before the candle is tested: a candle that then hits the stop closes the position the add made, and a stop the add moved (breakeven) is tested on that same candle.
- **Costs**: fee (fixed or % of notional, per trade or per unit) and **spread %**, so results aren't fantasy. A fee can be negative, for a maker rebate or a broker that pays per fill. The entry fee leaves the cash at the fill, as a broker debits it, so the equity, drawdown and adds of an open position are net of it.
- Settings with no meaning (no capital, a size or leverage of zero or below, a contract worth nothing, a grid that cannot be traded) are refused before the run, and so is a dataset with a candle that is not one (a high below the low, a price that is not a number), with that candle named.

### Sizing (advanced)

Beyond percent-of-equity and fixed quantity:

- **Risk per trade**: size so a stop-loss hit costs a fixed % of equity (needs a stop on the traded side).
- **Fractional Kelly**: size from the win rate and payoff of the strategy's last *N* signal trades, skipped ones included (a breakeven trade is neither a win nor a loss), scaled by your chosen fraction and capped; a warm-up size is used until the window fills. A losing stretch pauses entries without stopping them for good: they resume once the edge comes back.
- **Equity tiers**: a table of thresholds; the highest tier whose level is ≤ current equity sets the size.

### Portfolio (multi-asset)

Add several datasets and run one strategy across all of them on a **merged clock** (all locked to the same timeframe):

- An **alignment preview** shows the merged-clock length, the overlapping window, indicator warm-up bars (including the cumulative lookback of a chained custom indicator), and per-asset missing bars, all before you simulate.
- **Portfolio limits**: cap the number of open positions and total / per-asset exposure. A position held at an open keeps its slot there even when it closes later in that candle.
- **Sessions**: assets whose candles open at different hours (a crypto day at 00:00 UTC, a New York one at 13:30) act in that order. An order at the earlier open sizes and checks its limits on the later asset's previous close, not on an open that has not happened yet.
- A **per-asset breakdown** reports trades, net PnL, fees, win rate and exposure for each instrument.

### Grid strategy

A ladder of price levels between a lower and upper bound; each cell buys low and sells at the next level up, **long**, **short** or **neutral**. Size a fixed quantity per level or split a total budget across cells, with optional stops above/below the ladder. Results report fills, round trips and end inventory.

- **Neutral** trades both sides of the centre line: the cells below it buy and sell one level up, the cells above it sell short and buy back one level down. It needs an odd number of levels.
- A buy rests only on a line **below** the last close (a sell above it), and fills at the line, or at the open when the candle opens past it. Targets fill the same way.
- A **stop** below or above the ladder fills at its level (at the open on a gap), after the fills the price met on its way to it, then halts the grid.
- With no bounds, the ladder spans the range known so far: the lowest low and highest high of the candles before each one.
- Outside the trading window with *close*, the inventory goes at the open, before anything else in the candle.

### DCA (savings plan)

A third mode next to signal rules and the grid, for the way most money is actually invested: a **weighted basket**, bought over time, never rebalanced.

- **Weights, fixed.** Every euro deployed is split by the weights you set per ticker. Nothing is rebalanced, so a rule that fires on one asset of five deploys that asset's own share.
- **Money in.** The starting capital is bought in one shot at each asset's first bar. Everything after it is **new money**: a recurring contribution (per bar, day, week, month, quarter or year, invested on arrival or kept as cash), and buy rules that pay an amount in when their condition holds.
- **Buy rules**: a fixed amount, a % of cash, of the portfolio or of the cost basis, with a maximum number of fires and a cooldown, filled at the next bar's open.
- **Sell rules**: a % of the position, the whole position, a number of units or an amount, triggered by a **gain objective**, a condition, or both, with the proceeds kept as cash or withdrawn.
- **Conditions** are the engine's ordinary rule groups plus two families written for this mode: **market metrics** (drop from the high, rise from the low, change over N bars, change since the start, relative volume) and the **live position** (P&L %, drift since the last buy, average cost, units, value, weight %, cash %, drawdown). They are evaluated on a **weighted basket index** by default, or per asset, which then buys only the assets that hold.
- **Measures for a savings plan**, not for a strategy: drawdown and Sharpe on the **deposit-adjusted (time-weighted)** curve, so a deposit is not read as a rally; return on the money in; **IRR** for the money-weighted return; and a benchmark of the same total contributed deployed in one lump sum at the start.

Sizing, pyramiding and stops do not apply here: the plan's own rules decide every fill.

### Trading window

A **Filters** step decides *when* the strategy may open, on the clock you choose: a named time zone (`America/New_York`), which follows daylight saving, or a fixed UTC offset, which does not:

- **Weekdays** and **sessions** (several per day, an end before its start wraps past midnight).
- **Calendar**: trade only on given dates, or never on them. *Never* wins over *only*.
- Outside the window the position is either **kept** or **closed**, and pyramiding adds can be blocked too. Entries are gated; exits, stops and take-profits keep running on every bar.

### Costs & execution realism

- **Slippage**: a fixed number of ticks or a percent of price, applied to every fill.
- **Funding**: a constant annual rate on open notional for perp estimates (longs pay, shorts receive).
- **Circuit breakers**: halt trading after a max daily loss (for the day) or a max drawdown (for the run).
- **Instrument profile**: price tick, lot step, minimum quantity and contract multiplier, so sizes and prices snap to a realistic contract. Every fill and every stop, target, limit and grid line is rounded onto the tick, against the trader: a buy pays the tick above, a long's stop sits a tick lower.

### Order execution {#execution}

How stops and targets fill, always:

- A trade is tested against its stop and take profit **on the candle it opens in**, not from the next one.
- The **take profit is a limit order**: it fills at the target, with no slippage and no spread charged, once the side that deals reaches it (the bid for a long, the ask for a short), or only once the price **trades through** it if you pick that (Advanced, *Execution*). A candle that **opens past the target** fills it at that open, before anything else in the candle.
- The **stop loss is a stop order**: it fills at the stop, or at the open when the candle gaps past it, and pays spread and slippage.
- When one candle reaches **both** the stop and the target and nothing else says which came first, the stop wins.
- A trade closed inside a candle (stop, target, limit) is not reopened at that candle's open, a price from before the exit: a new entry waits for the next candle.
- Every signal decided at a close is acted on at the **next open**: an entry, the exit condition, and *exit when the entry stops holding*. Nothing fills at the close that produced its own decision.
- MAE and MFE count only what the trade lived through: from its fill to its exit, never the rest of the candle after it left.

Options in the **Advanced** step (*Execution*), all off by default:

- **Entry order**: market, or **limit**. A limit rests below the reference to buy and above it to sell, at an offset (percent, price distance or ATR multiple) from the signal candle's close or the next candle's open. It stays valid for the number of candles you set, fills on a touch or only when the price trades through it, and fills at its own price with no slippage (at the open when a candle opens past it). A signal that keeps holding does not move a resting order. Pyramid adds follow the same rule.
- **Exit order**: the same choice for signal exits (the exit condition, and *exit when the entry stops holding*). An exit limit that does not fill in time either **goes market** at the next open or is **cancelled**. A stop-and-reverse stays a market flip.
- **Check lower timeframe for SL/TP**: when a candle reaches both the stop and the target, or a limit fills mid-candle, the run reads a lower timeframe of the same instrument (same provider) inside *that candle only* to see what came first. It reads a stored dataset at that timeframe first, then candles downloaded by an earlier run, which are kept for later runs. *Auto* takes the finest stored timeframe, otherwise the finest the provider serves in one request per candle. Lower-timeframe candles that do not match the candle (different extremes) are not used.
- **Download missing lower-timeframe candles**: with this option, the candles a run needs and does not hold are downloaded from the provider, and only those. After a run, a notice says how many candles settled as a stop for want of them, with the cost (requests and time) and a checkbox to fetch them. A download up to about 4 minutes starts by itself in a progress window; a longer one waits for you, with the provider's rate limit and the quota left on its connector, since it can fail when not enough is left. The run then replays with the downloaded candles. When the provider refuses (no permission or subscription for that data, a key, a rate limit) or fails three times in a row, the run stops asking it and says so next to the results. A paper session with the option downloads the candles under the candle that just closed when it needs them, waiting a moment for the provider to publish them; without the option, or when they never come, the stop wins.
- **Use bid/ask prices**: fills read the bid and ask instead of the mid and the spread (a market buy pays the ask, a long's stop and target trigger on the bid). They are fetched from the provider only for the candles where a fill can happen, the easiest way it offers (see the table below), and kept for later runs. A short fetch starts by itself after the run, a long one asks first, with the same progress window as the lower timeframe. A candle without bid/ask uses the spread, and the result says how many did. Providers with no historical bid/ask leave the option off and say why.
- **Separate maker fee** (in the Costs block): limit fills (entry and exit limits, take profit) pay their own fee, negative for a rebate.
- **Raw prices**: stocks and ETFs are priced on bars adjusted for splits and dividends where the provider gives them (Yahoo, EODHD, Alpaca; Interactive Brokers bars come split-adjusted). Tick this to run on prices as traded. A stock dataset with no adjusted series is flagged next to the results.

The result shows limit orders placed, filled and expired, how many candles had both a stop and a target in reach and how they were settled (lower timeframe or worst case), and how many fills were priced on bid/ask.

#### Bid/ask and live price by provider {#bid-ask-providers}

| Provider | Historical bid/ask (backtest) | Live price (paper stops, targets, limits) | Live bid/ask (paper fills) |
|---|---|---|---|
| Capital.com | Bid and ask candles | Yes | Yes |
| OANDA | Bid and ask candles | No, checked at candle close | Not used |
| Interactive Brokers | Bid and ask candle series | Yes | Yes |
| FOREX.com | Bid and ask candles | No, checked at candle close | Not used |
| Alpaca | Quotes at the candle's open and close (stocks, ETFs, crypto) | Yes | Yes |
| Massive | Quotes at the candle's open and close (stocks, ETFs, options, forex) | Yes | Yes |
| Binance, Binance Futures, Kraken, OKX, Bitfinex, Bitget, Bybit, Bybit EU, Deribit, Gate.io, KuCoin, KuCoin Futures, MEXC, Coinbase | None, the spread applies | Yes | Yes |
| TradeStation, Yahoo, EODHD, Alpha Vantage | None, the spread applies | No, checked at candle close | Not used |

With quotes read at the open and the close, the spread inside the candle is their average around the candle's own high and low.

#### Paper trading on the live price {#paper-live}

A paper session reads its signals on closed candles, at its timeframe, and watches its levels on the live price in between:

- The **stop loss, the take profit and limit orders** (entry and exit) trigger on the first live price that reaches them, without waiting for the candle to close. The **trailing stop** still moves at each close, and the live price triggers it at the level set then.
- The fill is priced on the live bid/ask when the strategy uses bid/ask, otherwise on the live price with the spread and slippage of the settings. It is **alerted at once**, and the next run replays it on its candle as it happened, without a second alert.
- A provider with no live price for the instrument is named in the session dialog: there, stops, targets and limits are checked at candle close. If the live feed stops, or the app is offline, the candle takes over again and the session log says so.
- An entry is **alerted at the close that gives its signal**. A market order is announced at that close's price, with its size (taken on the equity marked at that close) and its **amount** of money for a broker that takes a notional order in fractional units, then fills at the next open: on the first live price when the provider streams one (its stop and target are then watched live from that fill), otherwise at that candle's open once it has closed. The price is updated without a second alert. A limit order is announced when it is placed (*Triggering limit order*), and its fill is alerted when it happens.
- A limit entry filled live gets its stop and target from the next run; until then that position is protected at candle close.

Paper trading runs the same engine as the backtest, with the same options: with *Download missing lower-timeframe candles* or bid/ask on, a run fetches what the candle that just closed needs, waiting a moment for the provider to publish it. The session row lists the limit orders it has working.

### Strategies and custom indicators

- **Named strategies**: save, search, duplicate and edit full strategy configurations.
- **Strategy versions** (opt-in): with versioning on in [Settings → Versioning](/config/settings#versioning), a saved strategy can be versioned from the history menu in the header. Save a version (dated, with an optional note), review what each step said, restore it (the restored state is saved as one new version, noted with the restored version's date, and keeps its name) or delete it. The history opens in a window with a search over notes and dates; the version matching the current strategy is marked. Notes are capped at 500 characters. Unsaved edits are saved before the version is taken. Turning versioning off for a strategy asks whether to keep or delete its versions. Deleting a strategy with versions asks whether to keep them; kept ones can restore it from **Deleted strategies with versions** in the Strategies tab. Agents with Backtest write access can do the same through [MCP](/config/ai-agents), saving a version after each update like a commit.
- **Custom indicators**: build your own from named steps, no code. Each step either applies a built-in indicator to a **source** (a price field or the output of an earlier step) or computes a **formula** referencing earlier steps by name (`@volume / @vol_avg`, with `+ − × ÷`, `min`, `max`, `abs`, `neg`, `clamp`). This lets you chain indicators: a Hull MA of an RSI, a MACD of an RSI, a smoothed volume ratio, and so on. Indicators that read full candles (ATR, Stochastic, ADX, VWAP…) only apply to the price, not to a derived step. The highlighted step(s) are the output. Custom indicators become operands in the rule editor alongside the built-ins, and the library is **shared with the chart**, which draws the same definition ([Custom indicators on the chart](#custom-indicators-on-the-chart)).
- **Searchable indicator picker**: pick indicators from a grouped, type-to-filter list (in both the rule editor and the custom-indicator builder) instead of scrolling one long dropdown.

### Date windows and parameter sweeps (API)

Two capabilities live on the API rather than in the form. They exist for the [assistant](/modules/agent) and for anyone driving the app over [MCP](/config/ai-agents):

- **Date-windowed runs.** `from` / `to` on a run (and on the alignment preview) restrict the simulated span, which is what walk-forward validation and regime slicing need: run 2019–2021, then 2022–2024, and compare. `to` is inclusive of the whole day.
- **`POST /api/backtest/sweep`**: run a parameter grid server-side and get **every trial back**, along with the trial count. Grid paths reach into arrays (`long.entry.conditions.0.left.period`), so indicator periods are sweepable; capped at 4 axes and 64 trials.

A sweep also returns a **deflated Sharpe**: the Sharpe the best of N *worthless* strategies would be expected to reach, given how much these particular trials varied. Compare the winner against that bar, not against zero: on real daily bars, the best of eight moving-average crossovers scoring 0.59 against a selection bar of 0.70 means *no evidence of an edge*, which the maximum alone would have hidden.

It also returns the **probability of backtest overfitting** (PBO) of the grid: the trials' equity curves are cut into 16 blocks, every half is used once as the in-sample and once as the out-of-sample set, and PBO is the share of splits where the in-sample winner ranks in the bottom half out of sample. The Quant [Compare](#quant) tab computes the same figure on saved runs.

### Optimizer

Take a finished run and **vary its parameters**: indicator lengths and thresholds per side, stops (trailing stop included), sizing, costs, portfolio limits, grid settings, and which weekdays to exclude (every subset is tried). Each parameter gets a from / to / step, and the header counts the variants as you widen them, capped so a grid stays finite.

- **Before it starts**, it estimates the cost from what past runs measured on your machine: milliseconds per variant, workers, total time. You can stop at any point and keep what has been computed.
- **Ranking** on the metric you pick (Sharpe, Sortino, return, net P&L, profit factor, win rate, expectancy, avg trade, max drawdown, trades); any column re-sorts afterwards. With an out-of-sample split, every ranked figure is the **in-sample** one, and the out-of-sample return is shown but never ranked: a winner picked on it would have seen it.
- **Analysis** shows the spread of the chosen metric across every variant: worst, average, best, and how many came out positive. A single good number means little if its neighbours are terrible.
- **Multiple-testing haircut**: measured on the candles per year the data actually has (a 24/7 market has about five times more hourly candles than a stock), the best Sharpe is shown against the **selection bar**, the Sharpe the best of that many *worthless* strategies would be expected to reach. Below the bar, trying this many variants is enough to explain the winner.
- Click a variant to read its **full backtest**, replayed from its own settings. Nothing is stored until you **keep** it in the history.

### Out-of-sample split

Split the data into an **in-sample** head and an **out-of-sample** tail; the strategy runs on both and the two stat blocks (return, profit factor, win rate, max drawdown, trades) are shown side by side. A large gap between the columns is a sign of overfitting.

### Paper trading

*A finished run, left running forward.* Press **Paper trade** on a result and the strategy keeps trading on paper, on a schedule, alerting the channels you pick. There is no second engine: every run re-simulates the window with the ordinary backtest and reports what changed, so a paper fill is by construction the fill the backtest would have shown for the same candles.

- **The strategy is frozen** as it ran, instruments included. Editing that strategy afterwards does not change a running session; the session's own form offers to update it or to start a copy when you save the strategy again.
- **The first run seeds the book.** Every round trip already in the window is recorded at once, summarized in a single event: opening a session does not fire a burst of alerts about history. Only what happens after that is a fill worth a message.
- **Window**: how much history each run feeds the engine (*Trailing bars*; a pinned start is set through the API only).
- The **Paper** tab lists the sessions with their status, next run, open positions and unread events, and each can be run now, paused, resumed or deleted. The event log is kept whether or not anything was sent, so what could not be delivered is still there when you come back.
- **Insights** on a session reads its closed paper trades, see [Insights](/modules/journal#insights).

#### Schedule

Every run, and its data, is on the candle's own clock.

- **Interval** (every N minutes), **daily**, **weekly**, **monthly** or **once**, in a real timezone.
- An interval fires on the **candle grid**, never on the second the session was created: every minute at :00, every 15 minutes at :00 / :15 / :30 / :45, hourly on the hour.
- The run **downloads the candle it waits for** into the session's own datasets. The candle in progress is never stored: what is read is the last *closed* one, a week from its Monday to the next, and a day only once a full day has passed since its stamp (a US daily candle lands after 04:00 UTC). A last candle stored before its period ended is read again. A candle the provider has not published yet is asked for again over a couple of seconds, and a period with no trade at all writes nothing, which simply means the next run has nothing new to simulate.
- A run that finds no new candle costs nothing: it does not simulate at all.

#### What is sent, and where

- **Notify**: on every run, only on a new fill, or never. *Every run* also reports the quiet ones, which is the only way to tell "nothing happened" from "the engine stopped running".
- **Summary**: none, or an hourly, daily or weekly message about the period. It never holds back a fill: entries and exits are sent on the run that produced them.
- **Alert on** entries, exits, or both, and optionally only for the **instruments** you name.
- **Channels**: the [notification channels](/config/settings#notifications) Backtest has been granted, all of them or the ones you pick. With no channel set up nothing is sent, and every event is still recorded in the app.

#### Custom messages

Three messages, three wordings, each with its own vocabulary: **Entry**, **Exit** and **Summary** (the periodic one). Leave a field empty and the built-in wording is used.

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
- A full **performance summary** (gross profit/loss, payoff ratio, largest win/loss, max consecutive wins/losses, average bars in trade…) and the complete **list of trades** with per-trade **MAE/MFE** (worst open loss / best open profit while in the trade), filterable, and exit reasons (signal faded, exit signal, stop-loss, trailing stop, take-profit, reversed, grid reset, window closed, data end).
- **Insights** on a saved run reads its trades for patterns worth acting on, see [Insights](/modules/journal#insights).
- **Save runs** by name and keep a history to compare strategies later. The **Reports** menu of a finished run exports it whole, per side and per asset: a **PDF** with every chart, or **Markdown** with the figures alone. Neither carries the trade list; the file is named after the strategy and the moment it was exported.

## Quant Tools {#quant}

Analytics on your datasets, your saved backtests and, for derivatives, straight from a provider. The tabs come in six groups.

Which tabs need what:

- **Asset** and **Multi-asset** read stored [Historical Data](#histdata) datasets.
- **Strategy** reads saved [Backtest](#backtest) runs.
- **Sizing** and **Calculators** take numbers you type (volatility targeting also reads a dataset).
- **Derivatives** reads an Interactive Brokers or Massive [connector](/config/connectors) granted to Quant.

### Asset

One dataset and one time window, shared by every tab of the group.

- **Risk**: annualized historical volatility, max drawdown, **Value at Risk** and **Conditional VaR** at your confidence. The tail beyond the VaR is estimated three ways (normal, Cornish-Fisher, a Generalized Pareto fit of the worst losses). A table lists the worst drawdowns with their depth, dates, and the bars it took to reach the trough and to recover, since one max drawdown figure hides how long the hole took to fill.
- **Statistics**: what kind of series this is.
  - Distribution against a normal of the same mean and volatility: skew, excess kurtosis, a QQ plot.
  - Serial dependence: autocorrelation of returns and of absolute returns, with Ljung-Box p-values. Returns rarely show any, absolute returns usually do: that is volatility clustering.
  - Trend or mean reversion: Hurst (R/S and DFA), variance ratios by horizon, and the price half-life.
  - Stationarity: ADF and KPSS, read together.
  - Whether the Sharpe ratio is distinguishable from luck: t-stat, probabilistic Sharpe, minimum track record length.
- **Volatility**:
  - Five estimators on the same bars. Close-to-close uses the closes only; Parkinson, Garman-Klass, Rogers-Satchell and Yang-Zhang also read the bar's range.
  - Their rolling paths.
  - A **volatility cone** that says whether today's reading is high or low for its horizon.
  - A **GARCH(1,1)** forecast with its persistence and shock half-life.
- **Regimes**:
  - A Gaussian hidden Markov model splits the returns into 2 to 4 states, calmest first, and shades the price chart by the most likely state.
  - Each state's return, volatility, time spent and typical stay.
  - The transition probabilities, and which state the market is most likely in now.
- **Events**: pick a condition (gap, big close, SMA or RSI cross, new N-bar high or low, streak, volume spike) and see what the market did next.
  - Forward returns at several horizons against the unconditional baseline over the same bars, with a p-value per horizon.
  - The average path around the event.
- **Seasonality**:
  - A month × weekday heatmap of mean return, volatility, bar range or volume, plus strips by month, weekday and (intraday only) hour.
  - Each cell shows its sample count and win rate. The hourly clock is **UTC**.

### Multi-asset

Two or more datasets with the same timeframe.

- **Portfolio**: correlation matrix, **efficient frontier** (a cloud of random allocations; click the max-Sharpe or min-volatility point to read its weights) and **risk parity**.
- **Pairs**:
  - Cointegration (Engle-Granger, and Johansen in both directions) and the spread with its hedge ratio, z-score and half-life.
  - Rolling correlation and beta.
  - Lead-lag correlation and Granger causality, to see whether one series moves first.
- **Basket**:
  - **PCA**: how many independent bets the basket really holds.
  - A correlation **dendrogram**: who moves together.
  - A **hierarchical risk parity** allocation.
  - A relative-strength table over 1, 3, 6 and 12 months.
  - A stress test that holds a weighting through each past crisis the data covers (2008, 2020, 2022 and others).
- **Regression**:
  - An asset's returns on one or more factor datasets (an index, bonds, gold, a sector).
  - Alpha with its t-stat, each factor's beta, R², tracking error and information ratio.
  - Up/down capture and a rolling beta.

Mixing asset classes is fine, including different providers: bars are matched by the **period** they belong to, not by the timestamp the provider stamped them with. A daily crypto candle opens at 00:00 UTC and a US equity one at the New York session start, and both are the same day. Two things follow, and the panel says which one applied:

- A basket that mixes a 24/7 market with an exchange-hours one is measured **weekly**. Aligned daily, the weekend move of the continuous asset would land on the same row as the other's Monday and understate how much they really move together.
- Annualization is **counted on the clock** rather than assumed: the same daily datasets are 252 periods a year on an exchange and 365 on a 24/7 market.

Intraday datasets are the exception: 4h bars anchored on a trading session and 4h bars anchored on the clock are 90 minutes apart, so a mixed-provider intraday basket is refused rather than approximated. Use daily datasets, or one provider for the whole basket.

### Strategy

Saved backtest runs. Each run is replayed on the server to regenerate its exact trades.

- **Monte Carlo**: resample a run's trades thousands of times (one by one, or in blocks to keep streaks together).
  - **Percentile bands** on the equity path, final equity and max drawdown.
  - The probability of ending at a loss.
  - A **risk of ruin**: the share of paths whose equity ever fell to a threshold you set.
  - The real equity curve drawn on top.
- **Trades**: what the trades are worth per unit of risk.
  - Expectancy in currency and in **R**, the R-multiple distribution, and **SQN** (on at most 100 trades, with Van Tharp's grading).
  - The **MAE/MFE** scatter: how much heat the winners took, how far the losers ran first.
  - 1R is the stop when the run has a percentage stop, otherwise the average loss, and the page says which. Grid and DCA runs record no MAE/MFE, so the scatter is left out for them.
- **Compare**: 2 to 20 runs on their common dates.
  - Rebased equity curves, a table of return, volatility, Sharpe and drawdown, and the correlation of their returns.
  - The **deflated Sharpe** of the best run, the others counting as the trials it was picked from.
  - The **probability of backtest overfitting** (PBO, by combinatorially symmetric cross-validation over 8 to 16 blocks). A PBO above 50% means the in-sample winner usually lands in the bottom half out of sample.

### Sizing

- **Position size**: from your stack, entry, stop and risk (percent or fixed), the **size, notional, margin, exposure and reward:risk**. It can **suggest stops** from a dataset (volatility, ATR, swing) and fill the entry from the last close.
- **Kelly**: the Kelly fraction from win rate and payoff, with half and quarter Kelly. Full Kelly maximizes long-run growth but swings hard; most traders size at half or quarter.
- **Vol targeting**:
  - Hold target volatility ÷ estimated volatility of the asset, with the estimate rolling or EWMA, capped at a maximum leverage.
  - Each bar is sized on the estimate known before it, so there is no look-ahead.
  - Shows the weight and the units to hold now for your equity, and the scaled track against holding the asset flat.
- **Risk of ruin**: the chance that a win rate, a payoff and a risk per trade reach a given drawdown, with a fixed amount or a fixed fraction risked per trade. Three answers:
  - The closed forms: Vince, for a fixed amount; the Cramér-Lundberg bound, for either sizing.
  - A simulation over the number of trades you set, with its standard error and the curve of ruin by trade count.

### Calculators

- **Options**: Black-Scholes-Merton price and greeks (vega and rho per point, theta per day), a binomial tree for American exercise with the early-exercise premium, and the **implied volatility** of a quoted price.
- **Futures basis**: from a spot and a futures price, the basis, the carry it implies per year, the implied repo, the fair value at your rate and yield, and the roll yield to the next contract.
- **Compounding**:
  - Where a capital and a return per period lead, with contributions.
  - The return needed to reach a target, and how many periods it takes.
  - The gain it takes to climb back from a drawdown.
- **Sharpe test**: for a Sharpe quoted without its data.
  - Is it distinguishable from zero, or from a benchmark?
  - How long a record does it need?
  - What is left of it once the number of strategies tried is accounted for (deflated Sharpe, with skew and kurtosis)?

### Derivatives

These read the provider directly instead of a stored dataset, so they need an **Interactive Brokers** or **Massive** connector granted to Quant.

A run is dozens of provider requests, paced by the provider. The page shows the requests done out of planned and what is being fetched. What was fetched is kept for six hours, so changing a rate or a roll rule recomputes without asking the provider again.

- **Futures curve**: the dated contracts of one product (`ES@CME` at Interactive Brokers, `ES` at Massive), expired ones included.
  - The **term structure** on the last finished session.
  - The **roll yield** between the front and the next contract over time.
  - A continuous series of holding the front and rolling, back-adjusted by ratio so it ends on today's price, next to the spliced one that jumps at each roll.
  - The roll is a calendar rule: the front is the nearest contract with more than *N* days left.
  - With a spot ticker (for example `SPX` as an index), it adds the basis, the carry `ln(F/S)` per year, the implied repo, and the mispricing against the fair value at the rate and yield you enter.
  - Massive curves use the settlement price of each session.
- **IV surface**: an underlying's option chain.
  - A few expiries spread between the minimum and maximum days you set, out-of-the-money strikes on each side, each price turned into a Black-Scholes-Merton implied volatility.
  - Smiles by expiry, the **ATM term structure**, 25-delta **risk reversal** and **butterfly**, and a check that ATM total variance never falls from one expiry to the next.
  - Interactive Brokers prices each option at the last hourly midpoint, taken the same hour as the underlying, so no options market-data subscription is needed. Massive prices each at the session close.
  - The page tells you the number of requests before you start: on a free Massive key (5 a minute) a chain takes several minutes.
- **Implied vs realized** (Interactive Brokers only): the underlying's 30-day implied volatility, up to ten years back, against the close-to-close volatility before and after each day.
  - The **volatility premium**: IV minus the volatility that followed.
  - **IV rank** and **percentile** over a lookback you choose.
  - How well IV forecast realized volatility (a regression of the realized that followed on IV).
  - The correlation of IV changes with price moves.

## Scanner {#scanner}

The Scanner watches a list of instruments on each closed bar and tells you when your conditions hold. It never places an order: a hit links to the chart. It needs Historical Data, reads through the [data connectors](/config/connectors) granted to the Scanner and notifies on the [channels](/config/settings#notifications) granted to it.

### Items and hives

- An **item** is one instrument on one connector at one timeframe. Add items from the symbol lookup, or import a chart list or a watchlist.
- A watchlist symbol is added only on an exact match on exactly one connector granted to the Scanner. The others are listed with the reason, to add from the lookup.
- A **hive** groups items, and an item can sit in several. An item takes the color of its first hive unless it has its own.
- A hive holds up to 200 items by default (1 to 2000, set in the Hives tab). Each item is read once per bar, so a big hive means many reads.

### Signals

A signal is a reusable condition, built one of four ways:

- **Signal**: conditions on prices and indicators, the same ones backtests and chart alerts use. Scripts are not accepted.
- **Event**: move from the open (of the session, or of the day in a timezone), move over a number of bars, new high, new low, gap, volume spike, range expansion. A threshold fires on the bar that crosses it, not on every bar past it. A window that starts mid-session does not know the open, so it stays silent.
- **Pattern**: engulfing, pin bar, inside bar, outside bar, doji, hammer, shooting star, three bar reversal, breakout, consolidation, higher highs and lows, lower highs and lows. The exact rule of each is shown next to it.
- **Time**: market sessions, weekdays, a from/to window or an exact time, in a timezone. Alone it is a schedule; with other signals it gates them. A bar counts at the time it closed.

**Test** runs a signal on the stored bars of one item and says on how many it held.

### Alerts

An alert scans one item or one hive with one or more signals, and every signal must hold on the same closed bar. For an OR, make two alerts.

- **Notify**: one digest per run, or one message per hit. A **cooldown** blocks a second hit on the same item for that many minutes.
- **Channels**: none picked means every channel granted to the Scanner. The in-app notification always arrives.
- **Run now** checks the alert on each item's last closed bar. It records nothing and sends nothing.
- A hit is recorded once per alert, item and bar, kept 90 days, and listed under **Hits** with its values and a link to the chart.
- A problem (deleted hive, hive over its cap, an item that cannot be read) is shown on the alert and notified once, then once more when it clears.

The scan runs every minute. An item is read only when a new bar has closed since the last one seen: from its stored dataset when that is fresh, otherwise with one small request through its connector.

### Board

The **Board** holds named boards, shown as tabs, and only the open one loads. A board is a set of views, and a view shows one hive as a **grid**, a **honeycomb** or small **line** charts (the first 48 items).

- **Color by**: change on the bar, change from the open, volume against its average, how recent the last hit is, or whether a signal is lit.
- **Sort by** orders the items, and **Hits only** hides the ones that have not hit.
- **Live** follows the live price while the page is open, up to 100 items per board. Over that, and for items whose connector cannot stream, the values stay on the last closed bar and the board says so.

In [demo mode](/guide/demo), alerts and settings are read-only, live is off and items are limited to stored datasets.
