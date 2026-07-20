<div align="center">

# OpenTraderWorld

**Your trading workspace, on your own machine.**

[![License](https://img.shields.io/badge/license-FSL--1.1--MIT-blue)](LICENSE)
[![Docs](https://img.shields.io/github/actions/workflow/status/G-OTW/OpenTraderWorld/docs.yml?branch=master&label=docs)](https://g-otw.github.io/OpenTraderWorld/)
[![Docker Compose](https://img.shields.io/badge/deploy-Docker%20Compose-2496ED?logo=docker&logoColor=white)](https://g-otw.github.io/OpenTraderWorld/guide/install)

A free, self-hosted platform bundling the tools traders and investors actually use —
trading journal, market data & backtesting, portfolios, news, planning — with your
data staying yours.

[**Documentation**](https://g-otw.github.io/OpenTraderWorld/) ·
[Live demo](https://demo.opentraderworld.com) ·
[Install in 2 minutes](https://g-otw.github.io/OpenTraderWorld/guide/install) ·
[Module tour](https://g-otw.github.io/OpenTraderWorld/modules/) ·
[Suggest a feature](https://github.com/G-OTW/OpenTraderWorld/issues)

</div>

---

## Try it without installing

A public sandbox runs at **[demo.opentraderworld.com](https://demo.opentraderworld.com)** — no
account, no install, seeded with realistic data so you can click through a used instance rather
than an empty one.

It is a *shared* sandbox: everything you type is visible to other visitors, the whole database is
wiped and re-seeded **every 15 minutes**, and settings, connectors and secrets are read-only. It
runs on a small free-tier server with free AI models, so expect it to be slower than a local
install. For anything real, install it — it takes two minutes.

## Quick start

Requires [Docker](https://g-otw.github.io/OpenTraderWorld/guide/docker) (macOS, Linux, or Windows).

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

The installer asks a few questions, generates strong secrets, pulls the prebuilt images
and starts the stack, creates your admin account and prints its password once. Open **http://localhost:5454**
and sign in. Full walkthrough (manual setup, headless servers, LAN/HTTPS/public exposure,
backups): [the documentation](https://g-otw.github.io/OpenTraderWorld/guide/install).

## Why OpenTraderWorld?

- 🆓 **Free.** No subscription, no paywall, no "pro tier". Free for everyone — personal
  or professional — the guiding principle: *get profitable before spending a dime.*
- 🔒 **Private & self-hosted.** Runs on your computer or server with Docker. No account,
  no cloud, no telemetry. After install it's localhost-only until *you* decide otherwise.
- 🧩 **Modular.** 27 modules — install only what you use, detach the rest.
- 🔁 **Continuously updated.** New modules and improvements land regularly. The app tells
  you when a new version is out; updating is a couple of commands shown in-app, and your
  data always survives.
- 🗳️ **Built with its users.** Feature ideas come from people using it — suggest yours,
  vote on others, and watch the roadmap follow. See [Contributing ideas](#-suggest-vote-contribute).
- 🌍 **7 languages.** English, French, German, Spanish, Italian, Portuguese, Chinese.
- 🤖 **AI built in, on your terms.** A built-in
  [assistant](https://g-otw.github.io/OpenTraderWorld/modules/agent) chats with your own data —
  bring your own provider (Anthropic or any OpenAI-compatible endpoint), with memory, skills and
  tool access scoped by token. And a built-in
  [MCP server](https://g-otw.github.io/OpenTraderWorld/config/ai-agents) lets *external* agents
  read or update your modules through the same token-scoped gateway. Both off by default; your
  keys are encrypted at rest and never reach the browser.
- 🔑 **Secrets stay sealed.** An encrypted [Vault](https://g-otw.github.io/OpenTraderWorld/config/settings#vault)
  holds the credentials the app needs on your behalf — encrypted at rest, write-only once saved,
  and plugged into any module by reference instead of being pasted again.

## Why not Edgewonk / TraderSync / Ghostfolio?

**Edgewonk** is a mature trading journal with trade analytics, and it's a great fit if you want a ready-made product. It is however a paid, closed-source subscription service — OpenTraderWorld is free, open-source, actively developed, and keeps all your data on your own machine.

**TraderSync** offers automatic broker imports and reporting as a cloud SaaS. If you'd rather not have your trading history on a third-party server (or pay a monthly fee), OpenTraderWorld gives you a self-hosted alternative — at the cost of manual/CSV entry instead of broker sync (we plan to expand import options in the near future).

**Ghostfolio** is also open-source and self-hosted, but it's a portfolio and wealth tracker geared toward buy-and-hold investing. OpenTraderWorld covers both active trade journaling and investing — per-trade legs and brackets, fee schedules, templates, and strategy stats.

**None of them talk to your data.** OpenTraderWorld ships a built-in AI assistant that reads and edits your journal, portfolios and notes through a permission-scoped gateway — with your own provider key, on your own machine. There's no equivalent in any of the three: your trading history never leaves your server to get there.

As a final word: OpenTraderWorld is free, ships with 27 modules, and has an ambitious roadmap.

## What's inside

Every module ships with the app — install only what you use, detach the rest.

### Trading

| Module | What it does |
|---|---|
| [Trading Journal](https://g-otw.github.io/OpenTraderWorld/modules/journal) | Trade log with templates, fee schedules, multi-currency FX and performance stats. |
| [Trading Routines](https://g-otw.github.io/OpenTraderWorld/modules/productivity#routines) | Recurring session checklists — pre-market prep, in-session discipline, post-market review. |
| [Mindset](https://g-otw.github.io/OpenTraderWorld/modules/productivity#mindset) | Daily mood & discipline check-ins with trends. |

### Market data & analysis

| Module | What it does |
|---|---|
| [Historical Data](https://g-otw.github.io/OpenTraderWorld/modules/market-data#histdata) | Download OHLCV history from multiple providers into local datasets. |
| [Historical Data Visualization](https://g-otw.github.io/OpenTraderWorld/modules/market-data#histviz) | Candle/OHLC/line/Renko charts with indicators, on your datasets. |
| [Backtest](https://g-otw.github.io/OpenTraderWorld/modules/market-data#backtest) | Rule-based strategy backtester with sizing, costs and full stats. |
| [Quant Tools](https://g-otw.github.io/OpenTraderWorld/modules/market-data#quant) | Risk/return analytics — VaR, correlation, efficient frontier, position sizing, Kelly. |

### Portfolios & money

| Module | What it does |
|---|---|
| [Watchlists](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#watchlists) | Symbol watchlists with live prices, day changes, sparklines and notes. |
| [Portfolio Tracker](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#portfolios) | Live portfolio value with buy/sell ledger and daily auto-priced holdings. |
| [MyWealth](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#wealth) | Net worth across all assets — accounts, property, crypto, valuables. |
| [Managers' Portfolios](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#mportfolios) | Superinvestor 13F holdings, browsable and snapshotable. |
| [Tax Calculator](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#taxcalc) | Trading & investing tax estimates from country templates. |
| [Subscriptions](https://g-otw.github.io/OpenTraderWorld/modules/portfolio#subscriptions) | Recurring subscriptions and spend overview. |

### News & research

| Module | What it does |
|---|---|
| [News](https://g-otw.github.io/OpenTraderWorld/modules/news-research#news) | RSS & JSON-API news aggregator with polling dashboards. |
| [Economic Calendar](https://g-otw.github.io/OpenTraderWorld/modules/news-research#economics) | Upcoming macro events. |
| [FinanceDatabase](https://g-otw.github.io/OpenTraderWorld/modules/news-research#findb) | Search 300,000+ instruments locally; organize favorites in folders. |
| [Resources](https://g-otw.github.io/OpenTraderWorld/modules/news-research#resources) | Bookmark library for books, links and references. |
| [Community Docs](https://g-otw.github.io/OpenTraderWorld/modules/news-research#community-docs) | Community-written guides, synced and readable offline. |

### Notes & organization

| Module | What it does |
|---|---|
| [Editor](https://g-otw.github.io/OpenTraderWorld/modules/productivity#editor) | Rich document editor with folders and table/kanban/gallery databases. |
| [ToDo](https://g-otw.github.io/OpenTraderWorld/modules/productivity#todos) | Task list with due dates and categories. |
| [Goals](https://g-otw.github.io/OpenTraderWorld/modules/productivity#goals) | Goals with metric tracking and deadlines. |
| [Calendar](https://g-otw.github.io/OpenTraderWorld/modules/productivity#calendar) | Personal event calendar; overlays reminders, todos and goals. |
| [RemindMe](https://g-otw.github.io/OpenTraderWorld/modules/productivity#remindme) | Reminders with in-app notifications and email/Telegram/Slack/Discord channels. |
| [Time Tracker](https://g-otw.github.io/OpenTraderWorld/modules/productivity#time) | Project timers with budgets and hourly-rate value. |
| [Prompt Store](https://g-otw.github.io/OpenTraderWorld/modules/productivity#prompt-store) | Searchable library of reusable AI prompts, tagged, rated and versioned. |
| [Webhooks](https://g-otw.github.io/OpenTraderWorld/modules/productivity#webhooks) | Private inbound URLs that turn external alerts into notifications. |

### AI

| Module | What it does |
|---|---|
| [Agent](https://g-otw.github.io/OpenTraderWorld/modules/agent) | Built-in AI chat assistant (bring your own provider) that can also act on your data via MCP, with memory, skills and external MCP servers. |

Full tour with details: [the module docs](https://g-otw.github.io/OpenTraderWorld/modules/).

## ⭐ Enjoying it? Help it grow

OpenTraderWorld is free and built in the open. If it's useful to you, the easiest way to
give back costs nothing:

- **Star the repo** — it's a quick signal that helps others discover the project.
- **Share it** — a word to a fellow trader, a link in your community, a mention anywhere
  helps more than you'd think.

No pressure — even just trying it out and telling a friend means a lot. 🙏
Got ideas or found a bug? See [Suggest, vote, contribute](#-suggest-vote-contribute) below.

## 💡 Suggest, vote, contribute

This project grows from user feedback:

- **Suggest a feature or module** — head to
  [opentraderworld.com/suggestions](https://opentraderworld.com/suggestions) and describe
  what you're missing and how you'd use it.
- **Vote** — 👍 the suggestions you want most on the website; popular requests get prioritized.
- **Report bugs** — open an [issue](https://github.com/G-OTW/OpenTraderWorld/issues) with the
  symptom and a few log lines.
- **Write for the community** — guides written in the app's Editor can be submitted for
  publication in the shared Community Docs library, readable by every install.

## Under the hood

| Layer | Tech |
|-------|------|
| Backend / core | **Rust** (Axum) — API, scheduler, background jobs |
| Frontend | **SvelteKit** single-page app |
| Data | **PostgreSQL** — the only place your data lives |
| Reverse proxy / TLS | **Caddy** — automatic HTTPS certificates |
| Deployment | **Docker Compose** — non-intrusive, quickly rebuilt |

```
OpenTraderWorld/
├── install.sh     # one-command installer (fetches deploy/, runs setup)
├── deploy/        # docker-compose stack, setup.sh, Caddy config
├── core/          # Rust workspace (API, store, scheduler, feeds…)
├── frontend/      # SvelteKit app (modules UI)
└── docs/          # documentation site (VitePress)
```

## Status

Actively developed — usable today, iterating fast. Check **Settings → Update app** in-app
to know when a new version is available.

## Disclaimer

This software is for **educational and informational purposes only** and is **not**
financial, investment, or trading advice. **USE IT AT YOUR OWN RISK.** The author and any
affiliates accept **no responsibility** for your trading or investment results, or for any
loss or damage arising from use of this software. Never risk money you cannot afford to
lose. There may be bugs — the software is provided **"as is", without warranty of any
kind**. You alone are responsible for how you use it and for any decisions you make.

## License

[FSL-1.1-MIT](LICENSE) (Functional Source License) — free for everyone, including
commercial use, self-hosting at work, modification, redistribution and forks. The only
thing not granted is a **Competing Use**: selling the software or offering it to others
as a paid product or hosted service. Each release automatically becomes **MIT** two
years after publication.
