# Portfolios & wealth

Independent modules for tracking what you watch, what you own, what it costs you, and what the tax bill might look like.

## Watchlists {#watchlists}

Named lists of symbols you want to keep an eye on: no positions, no ledger, just quotes.

- **Add symbols** by searching (crypto via CoinGecko, stocks/ETFs via Yahoo), start from a **curated template** (Crypto Top 10, Magnificent 7, US Index ETFs, Semiconductors), or **import a Portfolio Tracker portfolio**, where re-importing reconciles instead of duplicating.
- Each row shows the live USD price, **24h / 3d / 7d / 30d changes**, a **30-day sparkline**, the exchange, and a free-form **note** per symbol. Sort by any column, filter by name.
- **Auto-refresh** per list, from every minute to daily (15 min default). The page estimates the request rate and **warns before an interval risks free-API throttling**. Quotes are cached server-side, so reopening the page is instant and never hits the providers.

### Custom quote sources

The public sources (CoinGecko / Yahoo) work out of the box with no setup. If you have your own market-data account, plug in a **[data connector](/config/connectors)**, the same shared provider account [Historical Data](/modules/market-data#histdata) and the chart use, created once and granted to Watchlists. Each connector carries its own credentials (typed, or picked from the [Vault](/config/settings#vault)) and its own request limit.

- A list can **pin a connector as its default source**, and each symbol can override it: *follow list*, *auto*, or a specific connector.
- Per-symbol **provider tickers** (`BTCUSDT`, `AAPL.US`, …) are derived automatically and stay editable when a provider names a symbol differently.
- A quote that fails surfaces **on its own row**, so one bad symbol doesn't hide the rest of the list.

::: warning Know your plan's limits
A list backed by a custom source unlocks **5s–30s refresh intervals**. Those are fast enough to burn through an API plan quickly: excess calls fail and can get your key blocked. Watch the counters in **Settings → API rate**.
:::

### Price alerts

Any symbol can carry alerts, set from the bell on its row. Each one reads as one sentence you assemble left to right, *notify me when BTC moves ±5% from now*:

- **A level** (price above or below a value), or **a move** measured in **%** or in **$**, up, down or either way.
- A move is measured **from now**, or over a **rolling window** (1h, 4h, 12h, 1d, 3d, 7d, 30d).
- **One shot or repeating**, with a re-arm delay (5m to 1d) so a single swing cannot fire on every refresh.
- **Destinations**: the in-app inbox plus any [notification channel](/config/settings#notifications) you pick per alert. Watchlists can only target channels it has been granted.

Alerts are evaluated **server-side inside the refresh loop**, so they fire with the page closed and the browser shut.

### List description

A watchlist carries an editable description under its name, for what the list is actually for.

### Columns and views

- **Column picker**: the gear at the end of the table header picks the columns per list: market cap, volume, P/E, dividend yield, RSI(14), 52-week range, distance to the 52-week high, targets. A field the quote source does not serve shows **n/a**, never 0 (Yahoo serves the 52-week range and volume but no market cap, P/E or yield; CoinGecko serves market cap and volume).
- **52-week position bar**: where the price sits in its yearly range, shown by default. Crypto on the default source has no 52-week range; a year of stored daily candles fills it in when present.
- **Heatmap**: one tile per symbol, sized by market cap (equal size when unknown), colored by the 24h, 7d or 30d change.
- **Compare**: every symbol rebased to 100 over 1M, 3M or 1Y. 1M comes from the cached quotes; 3M and 1Y need daily candles stored in [Historical Data](/modules/market-data#histdata), and a symbol without them is listed as not covered with the fix.
- **Correlation**: the pairwise correlation of daily returns over the window, on the days both symbols traded. A pair with fewer than 10 overlapping days shows a dash.

### Row tools

- **Sections**: group rows (core, candidates, exits), collapse a group, drag a row onto another section, rename a section from its header.
- **Color flags**: a colored dot before the symbol, and one filter chip per flag in use.
- **Targets**: a buy and a sell target per symbol with the distance to each; the bell next to a target opens a level alert prefilled at that price.
- **Virtual position**: "if I had bought N at P on D", with the P/L and % under the price. Nothing is booked anywhere.
- **Events**: next earnings, ex-dividend and split dates per row and in an Upcoming strip, read from what [Fundamentals](/modules/fundamentals) has stored. Without stored data the strip says so.
- **News badge**: how many News & Research items name the symbol since the list was last marked read; a click opens the news filtered on it.
- **Open in Chart**: opens the symbol on the chart with its provider ticker. Crypto on the default CoinGecko source needs a quote source pinned first.
- **Add to portfolio**: adds the symbol to a chosen Portfolio Tracker portfolio with a first buy prefilled at the current price.

### Paste tickers and daily digest

- **Paste**: the Add symbols dialog takes a pasted list (commas, spaces or new lines). Each ticker is resolved on an exact match only, nothing is added before you confirm, an ambiguous ticker lists its matches to choose from and an unknown one says how to find it.
- **Daily movers digest**: from the list settings (gear next to the title), once a day at the chosen UTC hour, the three biggest gainers and losers over 24h go to the picked notification channels.

## Portfolio Tracker {#portfolios}

Live value of your actual holdings, one portfolio per account or theme.

- **Add assets** by searching (crypto coins or stocks/ETFs) and record **buy/sell operations** (date, quantity, price, fee, note). Realized and unrealized P/L, average cost and weights are computed from the ledger.
- **Trade currency per asset**: each asset declares the currency its operations are entered in, so a EUR-bought share isn't logged as if it were USD. Form labels follow that currency. Spot quotes stay in USD: cost basis and realized P/L convert at **each operation's own date** using the [Trading Journal](/modules/journal) FX rates, so a buy from three years ago keeps its historical rate. A popover in the form explains where each price comes from.
- **Auto-refresh**: a one-time reconcile step checks every holding against its price source; fix any that come up *unresolved* (or mark them manual) and enable **daily auto-update**, after which prices refresh in the background every day.
- Per portfolio: value, cost basis, unrealized/realized/total P/L, best & worst asset, **allocation** by asset or class, and a **value-over-time chart** (day/week/month/year) that fills in as refreshes accumulate.
- The **description** stays editable after creation, next to a foldable **investment thesis** note kept with the portfolio, for why you hold what you hold.

### Cash, income and costs

The ledger is not only buys and sells. **Cash & income** in the operations tab books a
**deposit**, a **withdrawal**, a **dividend**, **interest**, a **coupon**, a **fee** or a
**tax**, each in its own currency and with an optional fee withheld. Income can name the
holding that paid it, or nothing at all when it came from the account itself.

From those rows the portfolio gets a cash balance per currency, a total income, and a real
net worth (positions plus cash). Negative cash is shown, never clamped: it means margin, or a
ledger missing its deposits, and both are worth seeing.

### Analysis

The **Analysis** tab answers performance, risk and exposure in one place. Seven independent
views, each asking only for the data it needs, so *Book* renders instantly on a fresh install
while *Stress* pays for candles.

A view that cannot answer **says why and what to do about it**. It never shows a zero it did
not measure. No history yet, a book too short to annualize, no benchmark picked, no candles
for it, no targets set: each one is a sentence and a button, not an empty chart.

| View | Needs | Answers |
|---|---|---|
| **Book** | the ledger, nothing else | net worth, invested against cash, unrealized, realized, income, allocation by class |
| **Performance** | daily history | return, IRR, annualized, net contributions, per window |
| **Risk** | daily history | volatility, drawdown, Sharpe, Sortino, Calmar, best and worst periods |
| **Benchmark** | daily history and the benchmark's candles | what the index would have returned at your volatility, alpha, beta, capture |
| **Income & costs** | the ledger's cash rows | income collected, costs paid, annual drag, the curve without fees |
| **Allocation** | a target allocation | current against target, drift, the trades that close it |
| **Stress** | daily candles per holding | historical replay and factor shocks, with the covered share |

Every view runs over the same window picker: 1M, 3M, 6M, YTD, 1Y, 3Y, 5Y, All, or a custom
range. A window longer than your history is reported as **not covered**, with the days it
actually holds, rather than passed off as a full three years.

### Performance and risk

**Performance** reports a time-weighted return next to an IRR, and they answer different
questions. Time-weighted is what the investments did, deposit-adjusted, because a contribution
is not a rally. IRR is what **you** got, money-weighted, so timing your buys well shows up
there and nowhere else. Net contributions sit beside them, and a return over less than a fifth of a year
(about 73 days) is not annualized: multiplying six weeks by eight is a forecast, not a measure.

**Risk** reads the same curve: volatility, max drawdown, Sharpe, Sortino, Calmar, the share of
days that ended up, best and worst day, month, quarter and year, and every drawdown deeper
than 2 % with **how long it took to fill**. One still open is marked ongoing, with how far
below the last high you are and how many days it has been.

The annualization factor is **measured off your own curve**, not assumed. A stock book trades
about 252 days a year and a crypto book trades 365, and a mixed one is neither. Sharpe and
Sortino use the risk-free rate set in the measurement settings.

### Benchmark

Pick an instrument whose daily candles you have (SPY, QQQ, BTCUSDT) and the page answers the
only question that settles an argument: **what that index would have returned at your
volatility**, next to what you actually made. Beating the index by taking three times its risk
is not beating it.

Underneath: total and annualized return for both, volatility, max drawdown and Sharpe side by
side, then alpha, beta, tracking error, information ratio and up and down capture.

Your book is measured over the **benchmark's own sessions**. Compare a 24/7 portfolio to an
index day by day and every Monday of the index eats a weekend of yours, which quietly
understates your return.

### Income and costs

What you collected, what you paid, and what the paying cost you. Dividends, interest and
coupons on one side; trading fees and account fees on the other, with the annual drag as a
share of your average net worth.

The curve is drawn twice: as it happened, and the same book with the fee legs removed. Fees
are already inside your cost basis and your cash, so this is a comparison, not a subtraction
you could do yourself.

### Target allocation

Say what share of net worth each bucket should hold and how far it may drift before it counts
as off, in **Set targets**. An allocation has to add up to 100 %, and one bucket can be given
the remainder in a click. Saving an empty list turns the view off.

The view then shows current against target per bucket, the deviation, whether each one is in
its band, and the **trades that would close the gap**: buy this much of that, sell this much of
this. Anything you hold with no target is listed rather than ignored.

Display only. Nothing here places an order, and nothing rebalances on its own.

### Stress testing

Two engines, and both tell you how much of your book the number covers.

**Historical replay** applies the realized daily path of 2008, 2020, 2022, 2018 Q4 or the 2021
crypto top to what you hold today, using the instruments' own candles over those dates. No
model, no proxy. An instrument that did not exist then has no path: it is **named and
excluded**, never substituted for an index.

**Factor shock** moves a real instrument (S&P 500, Nasdaq, rates, EUR/USD, oil, credit
spreads) and reaches each holding through a **measured** sensitivity, fitted on its own
candles. A holding with no candles, too short a history or a fit with no explanatory power
gets **no beta**: it lands in *unexplained* with its weight, and the headline reads
"−11.8 % across the 74 % of the book that could be measured". Cash has a beta of zero, which is
usually the only diversification already in place.

A rate shock in basis points reaches a bond through a duration written on the scenario, so the
figure can be argued with. Recession, inflation spike and credit widening ship as editable
combinations of those legs.

A readiness panel lists what can be stressed and what cannot before you run anything, so a
thin result is explained in advance rather than after.

### Daily history

Every measure above except *Book* needs a curve, and snapshots only start the day you switch
the daily job on. A portfolio you have kept for six years would otherwise be measured from
last Tuesday. So the curve is **rebuilt from the ledger and stored candles**, day by day.

The gear in the Analysis tab opens **Measurement setup**:

1. **Name each asset's candle ticker** and the currency those candles are quoted in. A ticker
   in your ledger is not always the symbol your provider serves, and a share bought in EUR
   priced against USD candles is off by the exchange rate.
2. **Download missing candles**. They are queued as ordinary [Historical Data](/modules/market-data#histdata)
   jobs through the connectors granted to portfolios. If none of them carries one of your
   instruments, it is named, with what to grant.
3. **Rebuild the curve**. It reports the days rebuilt, and the days skipped because a holding
   had no candle that day. A day that cannot be valued is not stored, rather than stored wrong.

Editing an operation dated in the past marks the curve **stale from that date** and says so.
Rebuilding stays your call. Refreshing a portfolio also downloads what is missing and extends
the curve behind it, and tells you when a broker cannot serve one of your instruments.

### Import an operations ledger

**Import** in the portfolio header reads a broker export, a spreadsheet or another tracker (CSV, TSV, JSON, Parquet) and turns each row into one buy or sell operation. Same detection engine as the [journal import](/modules/journal#import-a-trade-book): headers in six languages plus value sniffing, delimiter, decimal and date conventions decided per column, unidentified columns left unmapped.

Three things it refuses to guess:

- **What a symbol is.** Every symbol in the file must point at an asset: one you already hold in this portfolio (matched automatically), a new asset to create, or *skip*. An unresolved symbol blocks the import, and assets are only created once you confirm.
- **A row that is neither a buy nor a sell nor a kind it recognizes** is listed as a row error instead of being invented into an operation. Dividends, deposits, withdrawals, fees and taxes **are** recognized, in six languages, and book as themselves. If the file states no kind at all, set the default once for the whole import.
- **A missing price** is derived from amount ÷ quantity and flagged, never silently filled.

Nothing is written until you validate the preview. Each import is one **batch**, revertible whole (created assets stay), and de-duplicated **per portfolio**, so re-importing the same file changes nothing.

### Import holdings from a broker

**From a broker** in the portfolio header reads a [broker account](/config/brokers)'s balance sheet instead of a file: the difference against your ledger is shown line by line, and each line you accept writes the one operation that makes the portfolio agree. Cost basis is used where the broker publishes one (Interactive Brokers) and asked for where it does not (the crypto exchanges publish a quantity and nothing else).

### Corporate actions, lots and transfers

**Corporate actions.** The **Corporate action** group of the cash form books a split, a reverse split or a spin-off on a holding. A split multiplies the units and divides the cost per unit by the same ratio; nothing is realized. A spin-off moves the share of cost basis you enter to the new holding, with the parent's purchase dates, and the parent keeps its units.

**Tax lots.** Click a symbol to open the holding. **Lots** lists each open purchase with its cost, value, days held and short or long term. The matching method, the holding threshold and the wash window come from your first [Tax Calculator](#taxcalc) profile; without one, FIFO is used with no term and no wash rule, and the screen says so.

**Transfer between portfolios.** **Transfer** in the holding moves units to another portfolio with their lots, cost basis and purchase dates. Nothing is realized on either side; the destination asset is created when missing.

### Income, thesis and classification

**Dividend calendar.** The **Income** tab forecasts annual income per holding (units × amount per payment × payments per year), shows yield on cost, the next ex and pay dates, and the income booked over the last twelve months beside the forecast. The quote sources do not provide dividend data: you enter the amount, frequency and dates in the holding, and a holding without them is listed as *no estimate*. The Dashboard has an **Upcoming income** card.

**Thesis and review date.** Each holding carries its own thesis and a review date. Theses due within a week show as a strip on the portfolios page.

**Sector and country.** Typed in the holding, or read from [Fundamentals](/modules/fundamentals) for a stock it knows. An ETF stays unclassified until you type it. The allocation donut groups by sector or country, and the unclassified share is its own slice.

### Across portfolios

**All portfolios.** With more than one portfolio, a block above the list sums every book in one currency: value, cost, P/L, cash, the symbols held in several portfolios with their combined weight, trade currencies and the largest lines.

**Compare.** Analysis → **Compare** draws this portfolio and another on one curve, both time-weighted and rebased to 100 on the first day both curves have.

### Return attribution and goals

**Contribution.** Under the performance table, bars show what each holding brought to the window's return, and to the deepest drawdown, from daily closes. The contributions add up to the return of the rebuilt book. A holding without daily closes in the window is named and the bars are not drawn.

**Loss harvesting.** Analysis → **Harvest** lists the lots under water, worst first, with the regime's wash window and a flag on any lot bought inside it. **Send to Tax Calculator** opens it with the loss prefilled.

**Goals.** Analysis → **Projection** takes a target value, a date and a monthly contribution, and shows progress, the contribution needed at the book's measured return, and a cone of 1000 paths drawn from its measured return and volatility. A curve under a fifth of a year is refused rather than extrapolated.

### Plans and notifications

**Planned trades.** **Plan** in the holding records a buy or a sell you intend at a price. The asset's own quote is checked every hour; the plan is marked reached once and can notify. **Book it** opens the trade form prefilled.

**Alerts and digest.** **Alerts** in the portfolio header sets the largest share one holding may take (a holding above it is flagged on its row), a notification when it is crossed, a notification when a target bucket leaves its band (also in the targets dialog), and a daily or weekly digest with value, change, top three movers and income since the last one. Each alert fires once per breach and re-arms when the book is back in line. Messages go to the [notification channels](/config/settings#notifications) granted to Portfolio Tracker.

## MyWealth {#wealth}

Net worth across **everything**: brokerage accounts, property, crypto, cash, valuables. Where Portfolio Tracker follows live-priced holdings, MyWealth tracks any asset you value yourself.

- Add assets with a name, type, currency and category, then **record value updates** over time (price × quantity, or a straight value, with a note). History is editable.
- **Net worth chart** by month or year, plus a per-category breakdown. Multi-currency with the same FX handling as the journal (assets without a rate are excluded and flagged).
- **Templates**, like the journal's: reserved price/quantity fields feed the value, custom fields hold notes per revision.
- **Owned or owed**: an asset can be a **liability** (a mortgage, a loan), so the headline is a real net worth. The page shows what you own and what you owe before it nets them.
- **Link a portfolio** instead of copying it: a linked portfolio is read live from the tracker every time, so its value in your net worth is never a stale copy.
- **Valuation age**: tell an asset how often it should be revalued and the page names the ones that have aged past it, oldest first. A house valued three years ago is wrong in silence, and this is what breaks the silence. A linked portfolio never goes stale: it is read, not remembered.
- **Loan schedule**: give a loan its amount, yearly rate, term and start date (and the payment if the lender's differs). Its balance follows the amortization formula, one value per due date is written for you, and Update lists the remaining payments with interest and principal.
- **Pair an asset with its debt**: an owned asset can name the liability held against it (a house and its mortgage); its detail shows equity and loan-to-value.
- **Yearly drift**: an assumed rate (+3 % for property, -15 % for a car) estimates the value after the last real valuation. The chart draws it dotted, it is never stored as a valuation, and the headline shows how much is estimated.
- **Broker account and wallet address**: the link dialog's Broker account tab adds a broker account's equity as an asset, read with its own credentials (the account must be granted to MyWealth in Settings, Brokers). A Wallet address asset reads a public BTC or ETH address on chain, priced in USD. Both are read at most once an hour and kept as that day's value; a failed read shows its error on the row, never a zero.
- **Goals**: target net worth and date, monthly expenses. The tab shows the growth measured from your curve (12 months of history needed), the projected date, the savings over 12 months net of market moves and drift, the savings rate and the FIRE number (25 × yearly expenses).
- **Under the chart**: Movers splits a period's change per category into revaluation, additions and removals; Then vs now puts the category table 1M, 1Y or 5Y ago next to today; Liquidity divides the categories you flag liquid by monthly expenses plus the Subscriptions forecast; Targets sets a share and band per category and shows the drift, with the Portfolio targets math.
- **Real net worth**: the Real toggle deflates the curve by a CPI index loaded in Fundamentals (US CPI by default, another in Settings). Without one the chart says so and offers to load US CPI from FRED.
- **Owner**: tag each asset with a household member, filter by owner, and see the breakdown by owner in the chart's Owners view.
- **Documents**: attach a deed, a contract or a statement (10 MB each) to an asset or one of its valuations.
- **Stale notice**: in Settings, a weekly notice lists the assets whose valuation is past its review interval and sends it through the channels granted to MyWealth.
- **Export and Tax Calculator**: the header menu exports assets and valuations as CSV; Tax Calculator sums the categories you pick and opens the calculator with the wealth-tax base filled in. The demo refuses exports and document downloads.

## Managers' Portfolios {#mportfolios}

Browse **superinvestors' 13F portfolios**: what famous fund managers hold, position sizes, recent activity, reported vs current value, 52-week ranges. Filter by manager or by ticker (*who holds AAPL?*).

Since 13F data changes quarterly, you can **save snapshots** of any portfolio and compare over time.

## Tax Calculator {#taxcalc}

Rough estimate of trading & investing taxes. **Not tax advice.**

- **Profiles** start from a **tax regime** per country and tax year: France (flat tax, or professional), Germany, the UK, the US, Australia, Switzerland, Spain and Italy. A regime sets the tax year's calendar, the cost basis method (weighted average, FIFO or the UK pool, overridable per profile), the wash rule where the country has one, allowances and rates, and shows its sources and the date its rules were checked. **Custom rates** cover any other country: flat or marginal rate, social charges, capital-gains and dividend allowances, optional holding relief. Any profile can add **wealth-tax brackets** (e.g. CH, ES, NO).
- **Loss carry-forward**: record each tax year's net result per pool (securities, crypto...). What is still carried into the year is worked out from those rows, oldest loss first, and dropped after the regime's carry period.
- Enter figures in **Summary** mode (start/end value, contributions, withdrawals, realized share) or **Itemized** mode (capital gains, derivative gains, crypto gains, dividends, interest, prior losses carried).
- **Load Trading Journal**: with the journal installed, one click loads a tax year's realized PnL, split into capital / derivative / crypto gains, converted at the year-end FX rate.
- **From a broker**: with a [broker account](/config/brokers) granted to the tax calculator, a tax year is read straight from the account, folded into closed positions and totalled per form line, each disposal converted at the rate of its own exit date.
- Results show the estimated tax with a per-item breakdown (taxable, allowance, base, rate) and the effective rate. Save scenarios to history to compare.

## Subscriptions {#subscriptions}

Every recurring cost in one list (trading tools, data feeds, streaming) with price, currency, billing frequency (weekly/monthly/quarterly/yearly) and category.

You get monthly/yearly **spend charts** (grouped or per subscription), the **monthly equivalent** of each subscription, next billing dates, and totals for next month. Pause a subscription to keep it listed without counting it.

### Lifecycle, reminders and budgets

- **Price history**: changing a price or currency logs it. The row's details show the price as a step line, and the monthly chart draws a dashed "cost creep" line: what everything running cost each month, at the price it had then.
- **Renewal reminders**: tick "Remind me before billing", set the days, and pick channels (none picked = every channel granted to Subscriptions). Each billing date is reminded once. A reminder needs a start date or a trial end.
- **Free trials**: tick "Free trial" and set its end date. Billing starts on that date, the row shows a badge, and trials about to convert are listed first.
- **Cancel**: the stop button sets an end date. The subscription leaves every total from that day, keeps its history, and the rail counts what it has saved since. Filter running / cancelled / all. Delete stays for mistakes.
- **Yearly switch**: for a plan billed more often, enter the yearly plan price. The "Switch saves" column shows 12 months at the current cadence minus the yearly price.
- **Usage rating**: a banner asks to rate each running service 1 to 5 every quarter. The "Worth it?" view ranks rated services by monthly cost per point, most expensive for their use first.
- **Budgets**: set a monthly cap per category in the display currency. A category above its cap shows in the rail and notifies once a month.
- **Calendar**: the Calendar view shows the month's billing dates and amounts. "Export .ics" downloads the next 12 months as calendar events (refused in the demo).
- **Payment method and card expiry**: record which card pays, filter by it, and see a badge when the card expires within 60 days or has expired.
- **Shared plans**: set your share in % of a family plan. Prices, the monthly equivalent and every total use your share.
- **Forecast**: the Forecast tab shows the next 12 calendar months of billings, per month, with the monthly average. MyWealth reads the same figures.
