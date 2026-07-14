# Modules overview

OpenTraderWorld is a set of **modules** — feature bundles you switch on individually in **Settings → Modules**. Everything ships with the app; installing a module makes it appear in the module switcher (top left) and on the dashboard. Detach a module to hide it again (its data is kept unless you delete it too).

## Dependencies

Three modules build on **Historical Data**'s downloaded datasets and need it installed:

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Everything else is independent, though some modules integrate when both are installed (e.g. Tax Calculator can import Trading Journal PnL; MyWealth can import Portfolio Tracker holdings; Calendar can display ToDos, Goals and Reminders).

## All modules

### Trading

| Module | What it does |
|---|---|
| [Trading Journal](/modules/journal) | Trade log with templates, fee schedules, multi-currency FX and performance stats. |
| [Trading Routines](/modules/productivity#routines) | Recurring session checklists — pre-market prep, in-session discipline, post-market review. |
| [Mindset](/modules/productivity#mindset) | Daily mood & discipline check-ins with trends. |

### Market data & analysis

| Module | What it does |
|---|---|
| [Historical Data](/modules/market-data#histdata) | Download OHLCV history from multiple providers into local datasets. |
| [Historical Data Visualization](/modules/market-data#histviz) | Candle/OHLC/line/Renko charts with indicators, on your datasets. |
| [Backtest](/modules/market-data#backtest) | Rule-based strategy backtester with sizing, costs and full stats. |
| [Quant Tools](/modules/market-data#quant) | Risk/return analytics — VaR, correlation, efficient frontier, position sizing, Kelly. |

### Portfolios & money

| Module | What it does |
|---|---|
| [Portfolio Tracker](/modules/portfolio#portfolios) | Live portfolio value with buy/sell ledger and daily auto-priced holdings. |
| [MyWealth](/modules/portfolio#wealth) | Net worth across all assets — accounts, property, crypto, valuables. |
| [Managers' Portfolios](/modules/portfolio#mportfolios) | Superinvestor 13F holdings, browsable and snapshotable. |
| [Tax Calculator](/modules/portfolio#taxcalc) | Trading & investing tax estimates from country templates. |
| [Subscriptions](/modules/portfolio#subscriptions) | Recurring subscriptions and spend overview. |

### News & research

| Module | What it does |
|---|---|
| [News](/modules/news-research#news) | RSS & JSON-API news aggregator with polling dashboards. |
| [Economic Calendar](/modules/news-research#economics) | Upcoming macro events. |
| [FinanceDatabase](/modules/news-research#findb) | Search 300,000+ instruments locally; organize favorites in folders. |
| [Resources](/modules/news-research#resources) | Bookmark library for books, links and references. |
| [Community Docs](/modules/news-research#community-docs) | Community-written guides, synced and readable offline. |

### Notes & organization

| Module | What it does |
|---|---|
| [Editor](/modules/productivity#editor) | Rich document editor with folders and table/kanban/gallery databases. |
| [ToDo](/modules/productivity#todos) | Task list with due dates and categories. |
| [Goals](/modules/productivity#goals) | Goals with metric tracking and deadlines. |
| [Calendar](/modules/productivity#calendar) | Personal event calendar; overlays reminders, todos and goals. |
| [RemindMe](/modules/productivity#remindme) | Reminders with in-app notifications and email/Telegram/Slack/Discord channels. |
| [Time Tracker](/modules/productivity#time) | Project timers with budgets and hourly-rate value. |
| [Prompt Store](/modules/productivity#prompt-store) | Searchable library of reusable AI prompts, tagged, rated and versioned. |
