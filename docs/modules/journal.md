# Trading Journal

Log every trade, in any currency, and get honest performance stats: equity curve, win rate, expectancy, profit factor, drawdown, Sharpe and more. The journal is organized in ten tabs: **Breakdown**, **Analytics**, **PnL Calendar**, **Trades**, **Strategies & capital**, **Tags**, **Templates**, **Fees & currency**, **Import**, **Pending tasks**.

## Categories

Trades live in **categories**: folders like *Crypto scalping* or *Long-term stocks*, each with its own color, capital and stats. Create them from the category bar; drag to reorder. Deleting a category deletes its trades.

## Set up capital

In **Strategies & capital**, give each category a **beginning stack** and record **refills** and **withdrawals** over time. This is what return, equity curve and drawdown are computed against; without it you still get PnL, but not returns.

You can also name **strategies** with their signal names (e.g. *Breakout, Pullback*). Tag trades with a strategy/signal and the Breakdown tab can filter by them: that's how you find out which setups actually pay.

## Templates

Templates drive the trade form. A prebuilt **standard trade** exists; create your own per market or style:

- **Reserved fields** (side, prices, quantity, fees, leverage, multiplier, currency, unit type…) feed the performance stats.
- **Custom fields** (text, numbers, choice lists…) are free-form: setup grade, market condition, whatever you track.
- A template can set a **default fee schedule**, pre-selected when logging from it (overridable per trade).

## Logging trades

From the Trades tab, pick a template (or the *Quick* one, which shows all fields) and fill the form. Two levels:

- **Simple**: one entry, one exit (or leave the exit empty for an open position).
- **Advanced**: scaling in/out with multiple **entry and exit legs** (each with its own price, quantity, fees, signal), plus **SL/TP brackets**. When a bracket triggers, check it and it folds into an exit leg.

The form previews average entry, net PnL and open quantity as you type. You can attach up to two images (chart screenshots), pick leverage and contract multiplier for derivatives, and write your own feedback on the trade.

**PnL is computed on read** and handles partially open positions. Cost basis is switchable between **weighted-average cost** (default) and **FIFO** (useful for tax export), and the choice is applied for real, both in the stats and in the trade form's live PnL preview.

## Fees

In **Fees & currency**, save **fee schedules**: fixed or percentage, charged per lot, unit, contract, or trade (e.g. *IBKR stocks: 0.05 % per trade*). Selecting a schedule on a trade auto-computes the fee; a manually entered fee always wins.

## Multi-currency & FX

Trades keep the currency you entered them in. The **breakdown currency** (display) is converted using a daily FX feed that backfills rates automatically each business day, carrying rates forward over weekends and holidays.

If a rate can't be fetched for some date, those trades are **excluded from converted totals** and show up in **Pending tasks**, where you enter the missing USD-based rates by hand (1 USD = … of that currency) and the trades count again.

## Breakdown (your stats)

Per category or across all, filterable by date range, ticker, side, asset class, strategy, signal and tag. The filter bar is shared with Analytics, so a scope set on one screen is the scope of the other, and the ticker list offers the symbols the journal actually holds:

- **Equity curve** in the display currency.
- Realized PnL · Return · Win rate · Trades (closed/open) · Expectancy · Profit factor · Avg win / Avg loss · Best / Worst trade · Max drawdown · Sharpe and Sortino · Total fees · Invested capital · Margin deployed · Return on margin.
- **Sharpe and Sortino** are computed on daily returns against the equity carried into each day, annualized, the same definition Analytics uses, so the two screens agree.

## Analytics (reading the book)

The whole book in ten tabs, over the same filters as the Breakdown:

- **Overview**: expectancy in **R** and total R (over the trades that carry a planned stop), risk per trade, Sharpe with Sortino beside it, max drawdown with the days spent below the peak, trading days won and lost, average day, current and best streak, then the **R distribution** and the cost of your tagged mistakes.
- **Distributions**: net PnL by holding time, entry hour, entry weekday and position size, and the trade count per profit bucket. Which hour of your day actually pays.
- **Behavior**: what you do around the edge, and what it costs. Profit concentration, size after a losing run, pace after a loss, how the day decays, and the trade after a win against the trade after a loss. See [Behavior analytics](#behavior).
- **Market data**: the candles behind your trades. MAE and MFE, exit efficiency, what was left on the table, stop distance in ATR, and results split by volatility regime and by trend at entry. See [Market data enrichment](#market-data).
- **Open risk**: the only tab about the present. What is still on the line, where that risk is concentrated, and whether five open lines are five bets or one. See [Open risk](#open-risk).
- **Scatter**: any two trade values plotted against each other (date, trade number, net, cumulative PnL, return on notional, R, holding time, size...), coloured by result, side, strategy, ticker or asset class, with a trend line and zoom.
- **Scatter 3D**: three trade values on three axes, plus the market session of the entry, in a box you turn and zoom by hand. It shows whether two variables only matter together.
- **Streaks 3D**: every winning and losing streak drawn trade by trade, in money or in R, from zero, so their shapes compare directly.
- **Breakdown**: performance grouped by strategy, symbol, tag, asset class or side, with trades, win rate, net, expectancy, avg R and profit factor per row.
- **Compare**: this day, week, month, quarter, year or a custom range against the one just before, row by row (net, trades, win rate, expectancy, avg R, profit factor, max drawdown, fees, trading days), above a strip of the last twelve periods.

Closed trades with no FX rate for their date are counted out loud rather than silently dropped.

## Behavior analytics {#behavior}

Performance stats say what the book returned. **Behavior** says how you got there, and which of your habits paid for it. Same closed trades as the rest of Analytics, same filter bar, no extra setup and no market data: it reads the trades you already logged.

Five cards, each answering one question.

### Where the profit comes from

The share of gross profit made by your five best trades, how many winners it takes to make half of it, and what the account looks like with those five taken out. Next to them a concentration index: 0 means every winner pays about the same, 1 means a single trade pays for the year. Mean win against median win shows the same skew from another angle, and a cumulative curve draws it.

The number to look at is the net without the top five. If it is negative, the edge rests on outliers you cannot schedule.

### Size after a losing run

Median entry notional grouped by what came before the trade: after a win, after one loss, after two, after three or more. Each row carries its trade count, win rate, expectancy, average R and net, so the escalation is priced, not just noticed.

Sizing up after two losses is the most expensive habit a journal catches. A flat row here is the discipline most books lose first.

### Pace after a loss

The median gap from one exit to the next entry, compared after a win and after a loss. A trade opened in far less than **your own** usual gap right after a loss is counted as a revenge trade, since a scalper and a swing trader do not share a clock. Those trades get their own line: how many, what they made, and what they average against everything else.

Days are compared too, a day carrying a loss against a clean one, on trade count.

### How the day goes

Average result by the rank of the trade inside its local day: first, second, third, fourth and after, with average R per rank and what each further trade of the day is worth. Plenty of books make their money before lunch and give it back after. This is where that shows.

### After a win, after a loss

The trade that **follows** a result, never the result itself. Trades, win rate, expectancy, average R, median size, average risk, median hold and median gap, side by side, with the three gaps that matter (expectancy, size, hold) called out underneath.

::: tip Statements have a floor
The sentence at the top of a card is only written above a **sample floor** (twenty closed trades in scope, eight on each side of a comparison) **and** an effect threshold. Under either, the cards still draw and are labelled a first look. Three trades cannot show a habit.
:::

## Market data enrichment {#market-data}

The trade log knows your entry, your exit and your stop. It does not know where price went while you were in, and that is where most of the useful answers are: whether your stops sit inside the noise, how much of each move you actually kept, and in which market conditions the strategy works.

The **Market data** tab loads the candles behind your own trades and measures them.

### Set it up once

Open **Sources** on the tab:

- **Candle size**: *Automatic* picks the coarsest timeframe that still leaves about twenty candles inside a typical position, read from your own median holding time. Pin one if you would rather decide.
- **Fetching**: *Off* measures only what is already stored, *On demand* downloads when you click, *Automatic* queues a new trade's missing window on its own and notifies you when it lands.
- **Source per asset type**: stocks, ETFs, crypto, forex and futures each pick a [data connector](/config/connectors) granted to the journal, or *Automatic*, which takes the first granted connector serving that type.

Nothing is downloaded behind your back, and the downloads are ordinary [Historical Data](/modules/market-data#histdata) jobs: same queue, same quota accounting, same job list.

### Discover, download, measure

Three buttons, in that order.

- **Discover** reads what the filtered trades need against what you already store, and writes nothing. Per instrument you get the trades in scope, the bars in store, the windows missing and a status: *ready*, *partial*, *missing*, *no source*, *unsupported*, *contract needed*, *no dates*.
- **Download missing** queues those windows and follows the batch. Only the holes are asked for, and a hole is asked of the bars rather than guessed from a gap: a daily equity series is missing every weekend, a 24/7 crypto series never is, so no gap width works on both.
- **Measure** walks each trade against its bars and stores the result.

Measuring is **incremental**. A stored measurement is redone when the trade was edited, when new candles landed, or when the grain changed. *Re-measure all* forces the whole scope, for when you change the candle size and want every trade on the same footing.

### Naming a futures contract

A stock is called the same thing everywhere. A futures contract is not, and your journal ticker usually names the root you trade rather than the contract your data provider serves.

So the journal asks once, instead of guessing. An instrument that needs it shows *contract needed*, and **Name the contract** takes the symbol the way your source spells it: `MNQU6` (what TWS shows and what you copy), or the root with its contract month, `MNQ.202609`, or `MNQ.202609@CME` when the root lists on several exchanges. Everything downstream uses that symbol.

Options are reported **unsupported** rather than matched to their underlying. Measuring an option trade against the stock's candles would produce numbers that look right and mean nothing.

### What you get

- **Excursions**: average MAE and MFE, in money and in units of the planned risk, with the median time from entry to each.
- **Exit efficiency**: the share of the best move you actually kept, and what was left on the table across every measured trade.
- **Are the stops too tight**: median stop distance in ATR at entry, how many stops sit under one ATR, and how many *winners* first went past 80 % of their risk. A stop inside the noise is a stop the market takes on its way to your target.
- **Are the targets too close**: winners that kept under half of the move offered, and what was showing at the best point against what came home.
- **How deep before it works**: the worst point of each trade bucketed in R, from 0 to 0.25R up to more than 1.5R. This is what tells you where a stop belongs.
- **By volatility regime** and **by trend at entry**: the same stats split calm / normal / volatile, and rising / flat / falling.

Regimes are **terciles of your own book**, not absolute thresholds. An absolute threshold would call every crypto trade volatile and tell you nothing about when your strategy works. Under twelve measured trades no regime is labelled at all.

The Scatter tab reads these too: MAE against R, efficiency against holding time, the cloud coloured by regime.

## Open risk {#open-risk}

Every other tab measures the past. **Open risk** measures what is still on the line right now.

A position is open when quantity remains, and it is counted at that **remainder**: a trade scaled out three quarters carries a quarter of the risk, not the risk it opened with.

### The headline

- **Gross and net exposure**, in money and as a share of the account, longs and shorts added and then netted.
- **At stake**: what you lose if every planned stop is hit. Positions with no logged stop are counted separately and named, since what they risk is unknown, not zero.
- **Open result**, marked to the last stored close, saying how many positions could actually be marked.
- **Effective bets**: how many independent positions your lines amount to, by size, and again at their measured correlations.

The positions table lists them biggest first, with side, open size, entry, stop, last price, value, what is at stake, its share of the total, open result and days held.

### Where the risk sits

Concentration by instrument, asset class, side or strategy, computed on **risk** when stops are logged and falling back to size when they are not (the panel says which). One instrument carrying half of what is at stake is a fact about your book that no equity curve shows.

### Are these separate bets

Five lines that move together are one position at five times the size. To answer that, the tab measures correlation on **daily candles already in store**, whatever grain the enrichment used, and never fetches anything.

Three numbers, and only the third describes your book:

- **Added up**: every stop hit at once, summed.
- **If independent**: what the risk would be if nothing moved together.
- **At these correlations**: what the book actually risks.

Their ratio is the **stacking** factor: 1.0 means genuinely separate bets, higher means the same bet several times. Instruments with no stored candles are named and left out of the matrix rather than assumed.

Warnings read as sentences: a pair moving at 0.9, a single instrument carrying too much, positions with no stop, a book thinner than it looks. When nothing is wrong, that is said too.

## What-if lab {#lab}

The **What-if lab** replays the closed trades of the open category under one change and sets the result next to what you actually did: the same figures before and after (trades, net, fees, win rate, per trade, profit factor, max drawdown, worst day), both cumulative curves on one axis, and every trade with its change. Where the change skips a trade, the curve stays flat.

One tab per change:

- **Exits**: other stops, targets and exit signals, see [below](#exit-lab).
- **Size**: every trade at 0.5 to 3 times its size, fees scaled with it. An optional **daily loss cap** blocks new entries for the rest of the day once the realized loss reaches it.
- **Filter**: keep or drop the trades opened in an entry time window (hours, weekdays), and keep only those where an indicator condition held on the last candle closed at the entry. Built-in indicators only, with the same math as backtests and chart alerts.
- **Reverse**: the same entries and exits with the side flipped, on every trade or only on those matching a filter. Fees stay as paid.
- **Fees**: the fees you paid against one of your own [fee schedules](#fees), charged the way the trade form charges them. With a contract family (micro and full futures), the trades move to the other contract, and contracts that do not make a whole one keep their own fees.

Nothing is guessed. A trade with no FX rate, no entry time for a window, or no stored candle for a condition is listed as **not measured**, with the reason, and left out of both sides. The one-sentence verdict appears from 20 closed trades. Below that, read the figures as a sketch.

### Exits {#exit-lab}

The **Exits** tab, also opened from **SL/TP signals** on the Trades tab, replays your closed trades under simple stop, target and exit rules (percentage, ATR and trailing stops, R targets, RSI, Bollinger, moving average and MACD exits, time exits...). Entry, size and fees stay yours, only the exit moves, so every figure is the price of the exit. It reads the candles stored by the [Market data](#market-data) tab, at most 500 trades per analysis.

- **How a signal applies**: *Added to my exit* lets a signal close a trade only before your real exit. *Replaces my exit* makes the signal the exit, held past your real exit as far as the stored candles go.
- Signals are ranked by net PnL, lowest drawdown, win rate, average R or profit factor against **My real exits**, with the best stop, best target and best stop + target called out, cumulative curves, and the trades behind each row.

### Strategy report card

**Report**, on a strategy in **Strategies & capital**, sums up the closed trades that carry it: trades, net, per trade, win rate, average R, profit factor and max drawdown, then the R distribution and the result by entry hour, weekday, volatility regime and instrument.

Under it, four standard what-ifs run on the same trades: a stop at the winners' median adverse move, a target at the 75th percentile of the favourable move, the worst entry hour dropped, and 1.5 times the size, under the daily loss cap you type on the card (none by default). The stop and the target need the excursions measured by [Market data](#market-data). **Open in lab** opens any of them in the lab with its settings filled.

## Insights {#insights}

The **Insights** button, on the journal header, on a saved backtest run and on a paper session, opens one page where a fixed set of researchers read the closed trades. Each finding is one sentence, with its evidence: how many trades, and what it changes in the result, the drawdown or the risk per trade.

What they look for:

- **Hours and weekdays**: the worst entry hour or day, and the best one, sized up 1.5 times.
- **Holding time**: losers held longer than winners, or winners given room to run.
- **Stop placement**: stops tighter than the winners' adverse moves need, or wider than they need.
- **Concentration**: one ticker or asset class dragging the book or carrying most of the trades. The sector is used only when a SEC industry code is stored for the company.
- **Fees**: what one of your fee schedules would have charged, also on the other contract of a family. With no schedule, it says so and links to Fees.
- **Size after losses** (backtest runs and paper sessions only, the journal's [behavior analytics](#behavior) already cover it): the position size after two losses in a row against the size after a win.
- **Long against short**: one side losing while the other earns.

How far to trust it:

- Nothing is stated under 20 closed trades. A finding also has to move at least 5% of the turnover (the sum of the absolute net results), with enough trades on each side of the comparison. *Nothing stands out* is a real answer.
- On the journal, the behavior and open-risk statements are listed next to the findings, unchanged.
- **Open in lab** opens the journal's what-if lab with the change filled, so you can check the claim trade by trade. On a run or a paper session, an hour or weekday finding offers **Add as filter**: the strategy opens with that trading window as an unsaved draft. It only ever narrows the window.
- On a run or a paper session, amounts are in the simulation's account currency, and fixed-amount fee schedules are not priced.
- A backtest run must be saved first. The journal source needs the Trading Journal module, runs and paper sessions need Backtest.

## Discipline tags

A **tag** is a rule you broke or honoured: *moved my stop*, *no setup*, *sized up after a loss*. Tick them on the trades where they apply and Analytics prices them: how many closed trades broke a rule, what those trades average against the clean ones, and the gap between the two. That is the **cost of mistakes**, in money.

## PnL Calendar

A month grid of daily realized PnL, green for up days and red for down, scaled to the month's largest day, with weekly totals down the side. Click a day to jump to its trades.

It also reads your **trading routines**. Attach the routines a category follows, over a period, and each traded day carries a dot: green when every routine due that day was ticked, red when none were, amber in between. A day you did not trade stays grey whatever the routines say, and a day owing no routine gets no dot at all. Hovering names each routine with its own mark.

The routines themselves live in [Trading Routines](/modules/productivity#routines) and are ticked there: the journal only records which ones a book runs, so the same habit is never written or ticked twice.

## Import a trade book

The **Import** view takes a trade book you keep somewhere else (a broker export, another journal, a spreadsheet) and turns it into journal trades. No per-broker parser: CSV, TSV, JSON and Parquet all go through the same detection.

**How it works.** Drop the file, the server proposes a mapping, you check it against a preview of real trades, then import.

- **Detection** reads the headers (en, fr, es, de, it, pt, plus common broker wording) *and* the values themselves. Delimiter, decimal separator and day-first vs month-first dates are decided per column. A column it cannot identify with confidence stays **unmapped** rather than guessed, and you assign it yourself.
- **Preview before writing.** The analyze step writes nothing: it returns the built trades, the totals and the per-row errors, and it re-runs on every mapping edit, so what you see is exactly what will be saved. The file's own P&L is cross-checked against the computed one.
- **Row shape.** A row is either a **round trip** (one row = one trade) or an **execution** (one row = one fill). Executions are grouped per instrument into positions with entry and exit legs; whatever is still open at the end is imported as an open trade.
- **Stacked statements.** An export packing several tables into one file (the IBKR style) is read section by section, with a picker to switch table or read the file flat.
- **Point value.** For each ticker found in the file, the import asks for the contract point value, since no export carries it. It is saved with the mapping.
- **Mappings.** Save a mapping and the next file from the same source is recognised by its header fingerprint and maps itself. Columns you correct by hand are remembered too.

::: tip A mapping is not a template
A journal **template** is the form you log a trade with by hand. An import **mapping** says which column of a foreign file is which trade field. They are listed separately and never mixed.
:::

### Pull from a broker

With a [broker account](/config/brokers) granted to the journal, **Pull from a broker** does the same import with no file: pick the account and a period, and the fills come back folded into positions, previewed before anything is written. There is no mapping to check, an API answers typed fields. Re-running a wider period refreshes the positions already imported instead of doubling them. Tick **Sync automatically** and the sync replays on its own at the interval you pick, from where the last one left off; the schedule is paused, re-timed or deleted from **Automator → Jobs**. **Schedule sync** on the Trades tab opens the same panel.

**Undoing an import.** Every import is one **batch**, listed with its date, file and trade count. *Revert* deletes exactly the trades it created. Imports are also de-duplicated **per category**, so re-importing the same file changes nothing (the same statement can still feed two categories, since a category is a book). *Keep, untrack* a batch to drop that protection and make its trades ordinary again.

## Export & reports

From the Trades tab you can export your data and generate a performance report:

- **CSV export**: the raw trades, for spreadsheets or tax software.
- **Periodic report**: a weekly or monthly performance summary (win rate, expectancy, fees, breakdown by strategy and category, equity curve), rendered to **Markdown or PDF**.

## Works with

- **Tax Calculator**: loads your realized journal PnL for a tax year, split into capital / derivative / crypto gains.
- **Historical Data**: the Market data and Open risk tabs read candles through the [data connectors](/config/connectors) the journal is granted, and download what they miss as ordinary jobs.
- **Trading Routines**: the PnL calendar shows, per traded day, whether the routines the category follows were ticked.
- **Dashboard**: a quick-trade widget logs a trade from the home page, and **Journal analytics** widgets show net PnL, win rate, the equity curve, open positions, behaviour insights and more.
- **RemindMe**: add a reminder from the journal page (e.g. weekly review).
