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

### Import an operations ledger

**Import** in the portfolio header reads a broker export, a spreadsheet or another tracker (CSV, TSV, JSON) and turns each row into one buy or sell operation. Same detection engine as the [journal import](/modules/journal#import-a-trade-book): headers in six languages plus value sniffing, delimiter, decimal and date conventions decided per column, unidentified columns left unmapped.

Three things it refuses to guess:

- **What a symbol is.** Every symbol in the file must point at an asset: one you already hold in this portfolio (matched automatically), a new asset to create, or *skip*. An unresolved symbol blocks the import, and assets are only created once you confirm.
- **A row that is neither a buy nor a sell** (dividend, deposit, transfer) is listed as a row error instead of being invented into an operation. If the file states no direction at all, set the default side once for the whole import.
- **A missing price** is derived from amount ÷ quantity and flagged, never silently filled.

Nothing is written until you validate the preview. Each import is one **batch**, revertible whole (created assets stay), and de-duplicated **per portfolio**, so re-importing the same file changes nothing.

## MyWealth {#wealth}

Net worth across **everything**: brokerage accounts, property, crypto, cash, valuables. Where Portfolio Tracker follows live-priced holdings, MyWealth tracks any asset you value yourself.

- Add assets with a name, type, currency and category, then **record value updates** over time (price × quantity, or a straight value, with a note). History is editable.
- **Net worth chart** by month or year, plus a per-category breakdown. Multi-currency with the same FX handling as the journal (assets without a rate are excluded and flagged).
- **Templates**, like the journal's: reserved price/quantity fields feed the value, custom fields hold notes per revision.
- **Import from Portfolio Tracker**: pull holdings in at today's price instead of re-typing them.

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
