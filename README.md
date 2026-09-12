<div align="center">

# OpenTraderWorld

**Your trading workspace, on your own machine.**

[![Website](https://img.shields.io/badge/website-opentraderworld.com-1f6feb)](https://opentraderworld.com)
[![License](https://img.shields.io/badge/license-FSL--1.1--MIT-blue)](LICENSE)
[![Docs](https://img.shields.io/github/actions/workflow/status/G-OTW/OpenTraderWorld/docs.yml?branch=master&label=docs)](https://g-otw.github.io/OpenTraderWorld/)
[![Docker Compose](https://img.shields.io/badge/deploy-Docker%20Compose-2496ED?logo=docker&logoColor=white)](https://g-otw.github.io/OpenTraderWorld/guide/install)

A free, self-hosted platform that bundles the tools traders and investors actually use: trading journal, market data and backtesting, portfolios, news, planning. Your data stays on your machine. And if you want it, your own AI agent runs the whole thing alongside you.

[**Website**](https://opentraderworld.com) ·
[Documentation](https://g-otw.github.io/OpenTraderWorld/) ·
[Install in 2 minutes](https://g-otw.github.io/OpenTraderWorld/guide/install) ·
[Module tour](https://g-otw.github.io/OpenTraderWorld/modules/) ·
[Suggest a feature](https://opentraderworld.com/suggestions)

### [▶ Try the live demo](https://demo.opentraderworld.com)

No account, no install, already full of realistic data. It is a shared sandbox, wiped every 15 minutes.

</div>

---

## Why OpenTraderWorld?

- 🆓 **Free.** No subscription, no paywall, no "pro tier". Free for everyone, whether you
  trade for a living or on the side. The idea behind it: *get profitable before spending a dime.*
- 🔒 **Private and self-hosted.** It runs on your computer or your server with Docker. No
  account, no cloud, no telemetry. Right after install it only answers on localhost, until
  *you* decide otherwise.
- 🧩 **Modular.** 25+ modules. Install what you use, detach the rest.
- 🔁 **Updated constantly.** New modules and improvements land regularly. The app tells you
  when a version is out, updating is a couple of commands it shows you, and your data always
  survives the upgrade.
- 🗳️ **Built with its users.** Feature ideas come from the people using it. Suggest yours,
  vote on other people's, watch the roadmap move. See [Contributing ideas](#-suggest-vote-contribute).
- 🌍 **7 languages.** English, French, German, Spanish, Italian, Portuguese, Chinese.
- 🤖 **AI built in, on your terms.** There is a built-in
  [assistant](https://g-otw.github.io/OpenTraderWorld/modules/agent) that chats with your own
  data. You bring the provider (Anthropic, or any OpenAI-compatible endpoint), and it gets
  memory, skills and tool access scoped by token. There is also a built-in
  [MCP server](https://g-otw.github.io/OpenTraderWorld/config/ai-agents), so *external* agents
  can read or update your modules through that same token-scoped gateway. Both are off until
  you turn them on. Your keys are encrypted at rest and never reach the browser.
- ⚙️ **Automate as much or as little as you like.** The
  [automator](https://g-otw.github.io/OpenTraderWorld/modules/automator) turns your routines
  into scheduled workflows: imports, reports, market scans, alerts, daily reviews. Any of them
  can hand a step to your own AI model, or be driven by it from end to end. You stay in charge
  the whole way: nothing runs, sends or writes anything you did not switch on yourself.
- 🔑 **Secrets stay sealed.** An encrypted [Vault](https://g-otw.github.io/OpenTraderWorld/config/settings#vault)
  holds the credentials the app needs on your behalf. Encrypted at rest, write-only once saved,
  and plugged into any module by reference so you never paste them twice.

## Quick start

You need [Docker](https://g-otw.github.io/OpenTraderWorld/guide/docker) (macOS, Linux or Windows).

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

The installer asks a few questions, generates strong secrets, pulls the prebuilt images and
starts the stack, creates your admin account and prints its password once. Then open
**http://localhost:5454** and sign in. If you want the long version (manual setup, headless
servers, LAN/HTTPS/public exposure, backups), it is all in
[the documentation](https://g-otw.github.io/OpenTraderWorld/guide/install).

> **If the page won't load over plain HTTP.** Browsers with HTTPS-only mode (Chrome/Edge
> HTTPS-First, Firefox HTTPS-Only, Safari) rewrite `http://` into `https://` and then cache
> that redirect, so a LAN install answers nothing at all. Open the address in a private
> window to confirm that is what is happening, then allow HTTP for that host. Or switch to
> the LAN + HTTPS mode in Settings → Network and get a real certificate.

## Try it without installing

There is a public sandbox at **[demo.opentraderworld.com](https://demo.opentraderworld.com)**,
also linked from the [website](https://opentraderworld.com). No account, no install, and it
comes seeded with realistic data so you land in an instance that looks used rather than empty.

One thing to keep in mind: it is *shared*. Everything you type is visible to other visitors,
the whole database is wiped and re-seeded **every 15 minutes**, and settings, connectors and
secrets are read-only. It runs on a small free-tier server with free AI models, so it is
slower than a local install. For anything real, install it. Two minutes.

## What's inside

Every module ships with the app. Install what you use, detach the rest.

### Trading

| Module | What it does |
|---|---|
| [Trading Journal](https://g-otw.github.io/OpenTraderWorld/modules/journal) | Trade log with templates, fee schedules, multi-currency FX and performance stats. |
| [Trading Routines](https://g-otw.github.io/OpenTraderWorld/modules/productivity#routines) | Recurring session checklists: pre-market prep, in-session discipline, post-market review. |
| [Mindset](https://g-otw.github.io/OpenTraderWorld/modules/productivity#mindset) | Daily mood and discipline check-ins, with trends over time. |

### Market data and analysis

| Module | What it does |
|---|---|
| [Historical Data](https://g-otw.github.io/OpenTraderWorld/modules/market-data#histdata) | Download OHLCV history from several providers into local datasets. |
| [Historical Data Visualization](https://g-otw.github.io/OpenTraderWorld/modules/market-data#histviz) | Candle, OHLC, line and Renko charts with indicators, live or on demand, on any instrument a connector serves. |
| [Backtest](https://g-otw.github.io/OpenTraderWorld/modules/market-data#backtest) | Signal, grid and DCA strategies with sizing, costs, an optimizer and paper trading forward. |
| [Quant Tools](https://g-otw.github.io/OpenTraderWorld/modules/market-data#quant) | Risk and return analytics: VaR, correlation, efficient frontier, position sizing, Kelly. |

### Portfolios and money

| Module | What it does |
|---|---|
| [Watchlists](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#watchlists) | Symbol watchlists with live prices, day changes, sparklines and notes. |
| [Portfolio Tracker](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#portfolios) | Live portfolio value with a buy/sell ledger and holdings priced daily. |
| [MyWealth](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#wealth) | Net worth across everything you own: accounts, property, crypto, valuables. |
| [Managers' Portfolios](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#mportfolios) | Superinvestor 13F holdings, browsable and snapshotable. |
| [Tax Calculator](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#taxcalc) | Trading and investing tax estimates from country templates. |
| [Subscriptions](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#subscriptions) | Your recurring subscriptions and what they cost you. |

### News and research

| Module | What it does |
|---|---|
| [News](https://g-otw.github.io/OpenTraderWorld/modules/news-research#news) | RSS and JSON-API news aggregator with polling dashboards. |
| [Mailbox](https://g-otw.github.io/OpenTraderWorld/modules/news-research#mailbox) | Newsletters, market news and broker mail read from your own IMAP mailbox, tracker-free. |
| [Economic Calendar](https://g-otw.github.io/OpenTraderWorld/modules/news-research#economics) | Upcoming macro events. |
| [FinanceDatabase](https://g-otw.github.io/OpenTraderWorld/modules/news-research#findb) | Search 300,000+ instruments locally, and file your favorites in folders. |
| [Resources](https://g-otw.github.io/OpenTraderWorld/modules/news-research#resources) | Bookmark library for books, links and references. |
| [Community Docs](https://g-otw.github.io/OpenTraderWorld/modules/news-research#community-docs) | Guides written by the community, synced and readable offline. |

### Notes and organization

| Module | What it does |
|---|---|
| [Editor](https://g-otw.github.io/OpenTraderWorld/modules/productivity#editor) | Rich document editor with folders and table/kanban/gallery databases. |
| [ToDo](https://g-otw.github.io/OpenTraderWorld/modules/productivity#todos) | Task list with due dates and categories. |
| [Goals](https://g-otw.github.io/OpenTraderWorld/modules/productivity#goals) | Goals with metric tracking and deadlines. |
| [Calendar](https://g-otw.github.io/OpenTraderWorld/modules/productivity#calendar) | Personal event calendar, with reminders, todos and goals overlaid on it. |
| [RemindMe](https://g-otw.github.io/OpenTraderWorld/modules/productivity#remindme) | Reminders with in-app notifications and email/Telegram/Slack/Discord channels. |
| [Time Tracker](https://g-otw.github.io/OpenTraderWorld/modules/productivity#time) | Project timers with budgets and hourly-rate value. |
| [Prompt Store](https://g-otw.github.io/OpenTraderWorld/modules/productivity#prompt-store) | Searchable library of reusable AI prompts, tagged, rated and versioned. |
| [Webhooks](https://g-otw.github.io/OpenTraderWorld/modules/productivity#webhooks) | Private inbound URLs that turn external alerts into notifications. |
| [Automator](https://g-otw.github.io/OpenTraderWorld/modules/automator) | Scheduled workflows: call your own API or any URL, ask an AI step, transform, notify. |

### AI

| Module | What it does |
|---|---|
| [Agent](https://g-otw.github.io/OpenTraderWorld/modules/agent) | Built-in AI chat assistant. Bring your own provider, switch model or prompt mid-conversation, and let it act on your data through MCP, with memory, skills and external MCP servers. Reachable from a floating chat on every page. |

The full tour, with details, is in [the module docs](https://g-otw.github.io/OpenTraderWorld/modules/).

## Backtesting: signals, grid, DCA

One engine, three ways to describe a strategy. No code.

- **[Signals](https://g-otw.github.io/OpenTraderWorld/modules/market-data#backtest).** Entry and
  exit rules from indicators, price and values, long or short, with stops, pyramiding, and
  sizing by percent of equity, risk per trade, Kelly or equity tiers. Build your own indicators,
  and the chart draws the same ones.
- **[Grid](https://g-otw.github.io/OpenTraderWorld/modules/market-data#grid-strategy).** A ladder
  of levels between two bounds, long, short or neutral. Each cell buys low and sells one level up.
- **[DCA](https://g-otw.github.io/OpenTraderWorld/modules/market-data#dca-savings-plan).** A
  weighted basket bought over time, never rebalanced: recurring contributions, buy tranches on a
  dip, sell rules on a gain objective. Measured like a savings plan, with IRR and a
  deposit-adjusted drawdown.

Run any of them on one instrument or a whole portfolio, with exposure limits and a per-asset
breakdown.

### Optimizer

Take a run and vary its parameters: indicator lengths, thresholds, stops, sizing, costs,
portfolio limits, grid settings, DCA amounts, excluded weekdays. Each gets a from / to / step.

- It **prices the job first**, from what past runs measured on your own machine. Stop at any
  point and keep what is computed.
- **Rank** on Sharpe, Sortino, return, profit factor, win rate, expectancy, drawdown, trades or
  out-of-sample return. Any column re-sorts after.
- **The spread matters more than the winner**: worst, average, best, and how many variants came
  out positive. One good number surrounded by terrible neighbours is noise.
- **A multiple-testing haircut** puts the best Sharpe against the bar the best of *N* worthless
  strategies would clear anyway. Below it, the winner is explained by the number of tries.
- Click any variant to read its full backtest. Nothing is stored until you keep it.

Add an **in-sample / out-of-sample split** and the two stat blocks sit side by side. A wide gap
between them is overfitting.

## Paper trading: the backtest, left running forward

Press one button on a run you like and it keeps going on new candles. There is no second
engine and no serialized simulator to drift: each tick reloads the window, runs the ordinary
backtest, and diffs the trades against what is already stored. A session opened from a result
reproduces that result exactly.

- **Its own schedule**: every N minutes, daily, weekly, monthly or once, in your timezone.
- **Live alerts** on entries and exits, or **a period summary** instead. Alert on every tick,
  only when something changed, or never.
- **Digest it**: hourly, daily or weekly, so a two-minute rule does not mean a two-minute ping.
- **Your wording, not ours.** Entry, exit and summary messages are templates you write, with a
  preview against a sample trade before you save.
- Anything the app could not deliver waits in the session inbox. You read it at the next login.

Filter alerts down to the tickers you care about, and pick which channels a session pushes to.

## Data connectors

One connector list for the whole app. A connector is a provider account, its credentials
(typed in or plugged from the encrypted Vault, write-only once saved), an optional request
limit, and the modules allowed to use it. Modules have no provider settings of their own.

| Provider | Credentials | Asset types | Symbol search | Live stream |
|---|---|---|---|---|
| **Binance** | none | crypto | yes | yes |
| **Kraken** | none | crypto | yes | yes |
| **Coinbase** | none | crypto | yes | yes |
| **Yahoo Finance** | none | equity, ETF, index, crypto | yes | no |
| **Alpha Vantage** | API key | equity, ETF, crypto, FX | yes | no |
| **EODHD** | API key | equity, ETF, FX, crypto | yes | no |
| **Alpaca** | key + secret | equity, crypto, options | yes | yes (based on your broker subscription) |
| **Massive (Polygon.io)** | API key | equity, ETF, options, futures, crypto, FX, index | yes | yes (based on your broker subscription) |
| **Interactive Brokers** | none (your own gateway) | equity, ETF, crypto, FX, index, futures, options | yes | yes (based on your broker subscription) |

Every provider connects directly, with an API key or none at all. Interactive Brokers is the
exception: you run IB Gateway or TWS yourself and the connector stores a host and a port.
Details in [the connectors guide](https://g-otw.github.io/OpenTraderWorld/config/connectors).

### Live feeds

| Provider | Price updates from | Live |
|---|---|---|
| **Binance** | trades | every timeframe |
| **Kraken** | trades | every timeframe |
| **Coinbase** | trades | every timeframe |
| **Alpaca** | trades | every timeframe |
| **Massive** | trades (quotes on FX, index values on indices) | every timeframe, futures excluded |
| **Interactive Brokers** | trades (midpoint on cash FX and crypto, which have no tape at IB) | every timeframe |

Missing a provider? [Ask for it](https://github.com/G-OTW/OpenTraderWorld/issues) and it gets
added.

## Notifications

These are the notification methods available today. More are planned: if your preferred one is
missing, [ask for it](https://github.com/G-OTW/OpenTraderWorld/issues).

| Channel | What you provide |
|---|---|
| **Email** | your own SMTP server |
| **Telegram** | a bot token and a chat id |
| **Slack** | an incoming webhook URL |
| **Discord** | an incoming webhook URL |

Secrets are sealed on save and never shown again. What can notify you: paper trading sessions,
watchlist alerts, reminders, finished or parked data downloads, new mail, journal jobs,
inbound webhooks, and any automator workflow. Everything is off until you switch it on, and
in-app notifications work without configuring a single channel.

## ⭐ Enjoying it? Help it grow

OpenTraderWorld is free and built in the open. If it is useful to you, the easiest way to give
back costs nothing:

- **Star the repo.** It is a quick signal, and it helps other people find the project.
- **Share it.** A word to a fellow trader, a link in your community, a mention anywhere. It
  helps more than you would think.

No pressure. Even just trying it out and telling a friend means a lot. 🙏
Got ideas, or found a bug? See [Suggest, vote, contribute](#-suggest-vote-contribute) below.

## 💡 Suggest, vote, contribute

This project grows on user feedback:

- **Suggest a feature or a module.** Head to
  [opentraderworld.com/suggestions](https://opentraderworld.com/suggestions) and describe what
  you are missing and how you would use it.
- **Vote.** 👍 the suggestions you want most on the website. Popular requests get prioritized.
- **Report bugs.** Open an [issue](https://github.com/G-OTW/OpenTraderWorld/issues) with the
  symptom and a few log lines.
- **Write for the community.** Guides written in the app's Editor can be submitted for
  publication in the shared [Community Docs](https://opentraderworld.com/docs) library, where
  every install can read them.

## Project links

| Where | What you'll find |
|---|---|
| [**opentraderworld.com**](https://opentraderworld.com) | Project home: module overview, community guides, roadmap voting, demo. |
| [opentraderworld.com/tools](https://opentraderworld.com/tools) | The 25+ modules by domain, in plain language. |
| [opentraderworld.com/docs](https://opentraderworld.com/docs) | Community Docs: trading guides written by users, synced into every install. |
| [opentraderworld.com/suggestions](https://opentraderworld.com/suggestions) | Suggestions and polls: propose features, vote on the roadmap. |
| [demo.opentraderworld.com](https://demo.opentraderworld.com) | Live shared demo, reseeded every 15 minutes. |
| [Documentation](https://g-otw.github.io/OpenTraderWorld/) | Install, configuration and module reference. |
| [GitHub](https://github.com/G-OTW/OpenTraderWorld) | Source, releases, issues. |

## Under the hood

| Layer | Tech |
|-------|------|
| Backend / core | **Rust** (Axum): API, scheduler, background jobs |
| Frontend | **SvelteKit** single-page app |
| Data | **PostgreSQL**, the only place your data lives |
| Reverse proxy / TLS | **Caddy**, automatic HTTPS certificates |
| Deployment | **Docker Compose**, non-intrusive and quickly rebuilt |

```
OpenTraderWorld/
├── install.sh     # one-command installer (fetches deploy/, runs setup)
├── deploy/        # docker-compose stack, setup.sh, Caddy config
├── core/          # Rust workspace (API, store, scheduler, feeds…)
├── frontend/      # SvelteKit app (modules UI)
└── docs/          # documentation site (VitePress)
```

## Status

Actively developed. It is usable today and moving fast. Check **Settings → Update app** in the
app to know when a new version is out.

## Disclaimer

This software is for **educational and informational purposes only**. It is **not** financial,
investment, or trading advice. **USE IT AT YOUR OWN RISK.** The author and any affiliates
accept **no responsibility** for your trading or investment results, or for any loss or damage
arising from use of this software. Never risk money you cannot afford to lose. There will be
bugs: the software is provided **"as is", without warranty of any kind**. You alone are
responsible for how you use it and for the decisions you make with it.

## License

[FSL-1.1-MIT](LICENSE) (Functional Source License). Free for everyone, including commercial
use, self-hosting at work, modification, redistribution and forks. The one thing it does not
grant is a **Competing Use**: selling the software, or offering it to others as a paid product
or a hosted service. Each release turns into plain **MIT** two years after it is published.
