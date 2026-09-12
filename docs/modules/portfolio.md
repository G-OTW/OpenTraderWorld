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

Every view runs over the same window picker: 1M, 3M, 6M, YTD, 1Y, 3Y, 5Y, all, or a custom
range. A window longer than your history is reported as **not covered**, with the days it
actually holds, rather than passed off as a full three years.

### Performance and risk

**Performance** reports a time-weighted return next to an IRR, and they answer different
questions. Time-weighted is what the investments did, deposit-adjusted, because a contribution
is not a rally. IRR is what **you** got, money-weighted, so timing your buys well shows up
there and nowhere else. Net contributions sit beside them, and a return under two months is
not annualized: multiplying six weeks by eight is a forecast, not a measure.

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

**Import** in the portfolio header reads a broker export, a spreadsheet or another tracker (CSV, TSV, JSON) and turns each row into one buy or sell operation. Same detection engine as the [journal import](/modules/journal#import-a-trade-book): headers in six languages plus value sniffing, delimiter, decimal and date conventions decided per column, unidentified columns left unmapped.

Three things it refuses to guess:

- **What a symbol is.** Every symbol in the file must point at an asset: one you already hold in this portfolio (matched automatically), a new asset to create, or *skip*. An unresolved symbol blocks the import, and assets are only created once you confirm.
- **A row that is neither a buy nor a sell nor a kind it recognizes** is listed as a row error instead of being invented into an operation. Dividends, deposits, withdrawals, fees and taxes **are** recognized, in six languages, and book as themselves. If the file states no kind at all, set the default once for the whole import.
- **A missing price** is derived from amount ÷ quantity and flagged, never silently filled.

Nothing is written until you validate the preview. Each import is one **batch**, revertible whole (created assets stay), and de-duplicated **per portfolio**, so re-importing the same file changes nothing.

## MyWealth {#wealth}

Net worth across **everything**: brokerage accounts, property, crypto, cash, valuables. Where Portfolio Tracker follows live-priced holdings, MyWealth tracks any asset you value yourself.

- Add assets with a name, type, currency and category, then **record value updates** over time (price × quantity, or a straight value, with a note). History is editable.
- **Net worth chart** by month or year, plus a per-category breakdown. Multi-currency with the same FX handling as the journal (assets without a rate are excluded and flagged).
- **Templates**, like the journal's: reserved price/quantity fields feed the value, custom fields hold notes per revision.
- **Owned or owed**: an asset can be a **liability** (a mortgage, a loan), so the headline is a real net worth. The page shows what you own and what you owe before it nets them.
- **Link a portfolio** instead of copying it: a linked portfolio is read live from the tracker every time, so its value in your net worth is never a stale copy.
- **Valuation age**: tell an asset how often it should be revalued and the page names the ones that have aged past it, oldest first. A house valued three years ago is wrong in silence, and this is what breaks the silence. A linked portfolio never goes stale — it is read, not remembered.

## Managers' Portfolios {#mportfolios}

Browse **superinvestors' 13F portfolios**: what famous fund managers hold, position sizes, recent activity, reported vs current value, 52-week ranges. Filter by manager or by ticker (*who holds AAPL?*).

Since 13F data changes quarterly, you can **save snapshots** of any portfolio and compare over time.

## Tax Calculator {#taxcalc}

Rough estimate of trading & investing taxes. **Not tax advice.**

- **Profiles** start from **country templates** (individual or professional) and remain fully editable: marginal income rate, social charges, capital-gains and dividend allowances, optional **wealth-tax brackets** (e.g. CH, ES, NO), long-term relief tiers.
- Enter figures in **Summary** mode (start/end value, contributions, withdrawals, realized share) or **Itemized** mode (capital gains, derivative gains, crypto gains, dividends, interest, prior losses carried).
- **Load Trading Journal**: with the journal installed, one click loads a tax year's realized PnL, split into capital / derivative / crypto gains, converted at the year-end FX rate.
- Results show the estimated tax with a per-item breakdown (taxable, allowance, base, rate) and the effective rate. Save scenarios to history to compare.

## Subscriptions {#subscriptions}

Every recurring cost in one list (trading tools, data feeds, streaming) with price, currency, billing frequency (weekly/monthly/quarterly/yearly) and category.

You get monthly/yearly **spend charts** (grouped or per subscription), the **monthly equivalent** of each subscription, next billing dates, and totals for next month. Pause a subscription to keep it listed without counting it.
